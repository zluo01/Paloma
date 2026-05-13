use crate::{Result, StorageError};
use log::error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug)]
pub enum WriterEvent {
    Append { session_id: Uuid, entry: FileEntry },
    Close { session_id: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "type")]
    pub t: String,
    pub payload: Value,
}

pub struct SessionWriter {
    root_dir: PathBuf,
    /// lazy storage on files that is opened
    open_files: HashMap<Uuid, tokio::fs::File>,
    event_rx: mpsc::Receiver<WriterEvent>,
}

impl SessionWriter {
    pub fn new(root_dir: PathBuf) -> (Self, mpsc::Sender<WriterEvent>) {
        let (tx, rx) = mpsc::channel(32);

        let writer = Self {
            root_dir,
            open_files: HashMap::new(),
            event_rx: rx,
        };

        (writer, tx)
    }

    pub async fn run(&mut self) {
        while let Some(event) = self.event_rx.recv().await {
            if let Err(err) = self.handle_event(event).await {
                error!("session writer error: {err}");
            }
        }
    }

    async fn handle_event(&mut self, event: WriterEvent) -> Result<()> {
        match event {
            WriterEvent::Append { session_id, entry } => {
                let file = self.open_file(session_id).await?;
                let mut line = serde_json::to_string(&entry)?;
                line.push('\n');
                file.write_all(line.as_bytes()).await?;
                file.flush().await?;
            }
            WriterEvent::Close { session_id } => {
                self.open_files.remove(&session_id);
            }
        }
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
