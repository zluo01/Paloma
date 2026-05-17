use crate::{Result, StorageError};
use log::error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub async fn read_session_entries(root_dir: &Path, session_id: Uuid) -> Result<Vec<FileEntry>> {
    let path = root_dir.join(format!("{session_id}.jsonl"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut entries = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<FileEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => error!("skip malformed entry in {path:?}: {e}"),
        }
    }
    Ok(entries)
}

#[derive(Debug)]
enum WriterEvent {
    Append {
        session_id: Uuid,
        entry: FileEntry,
        reply: oneshot::Sender<Result<()>>,
    },
    Close {
        session_id: Uuid,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    ResponseItem,
    EventMsg,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "type")]
    pub t: EntryType,
    pub payload: Value,
}

pub struct SessionWriterClient {
    event_tx: mpsc::Sender<WriterEvent>,
}

pub struct SessionWriter {
    root_dir: PathBuf,
    /// lazy storage on files that is opened
    open_files: HashMap<Uuid, tokio::fs::File>,
    event_rx: mpsc::Receiver<WriterEvent>,
}

impl SessionWriter {
    pub fn new(root_dir: PathBuf) -> (Self, SessionWriterClient) {
        let (tx, rx) = mpsc::channel(scry_config::SESSION_WRITER_CHANNEL_CAPACITY);

        let writer = Self {
            root_dir,
            open_files: HashMap::new(),
            event_rx: rx,
        };

        (writer, SessionWriterClient { event_tx: tx })
    }

    pub async fn run(&mut self) {
        while let Some(event) = self.event_rx.recv().await {
            self.handle_event(event).await;
        }
    }

    async fn handle_event(&mut self, event: WriterEvent) {
        match event {
            WriterEvent::Append {
                session_id,
                entry,
                reply,
            } => {
                let result = self.do_append(session_id, entry).await;
                let _ = reply.send(result);
            }
            WriterEvent::Close { session_id } => {
                self.open_files.remove(&session_id);
            }
        }
    }

    async fn do_append(&mut self, session_id: Uuid, entry: FileEntry) -> Result<()> {
        let file = self.open_file(session_id).await?;
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        file.write_all(line.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    async fn open_file(&mut self, session_id: Uuid) -> Result<&mut tokio::fs::File> {
        if !self.open_files.contains_key(&session_id) {
            let path = self.path_for(session_id);
            let file = tokio::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
                .await?;
            self.open_files.insert(session_id, file);
        }
        self.open_files
            .get_mut(&session_id)
            .ok_or_else(|| StorageError::NotFound(session_id.to_string()))
    }

    fn path_for(&self, session_id: Uuid) -> PathBuf {
        self.root_dir.join(format!("{session_id}.jsonl"))
    }
}

impl SessionWriterClient {
    pub async fn append_file(&self, session_id: Uuid, entry: FileEntry) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(WriterEvent::Append {
                session_id,
                entry,
                reply: reply_tx,
            })
            .await
            .map_err(|_| StorageError::ChannelClosed)?;
        reply_rx.await.map_err(|_| StorageError::ChannelClosed)?
    }
}
