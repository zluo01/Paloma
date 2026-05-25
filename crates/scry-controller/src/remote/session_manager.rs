use crate::entity::{ChatRenderEvent, RenderEvent};
use log::{debug, error};
use scry_provider::entity::{ChatEvent, ProviderId};
use scry_storage::db::Storage;
use scry_storage::session::{read_session_entries, EntryType, FileEntry, SessionWriterClient};
use scry_storage::StorageError;
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct SessionUpdate {
    pub session_id: Uuid,
    pub event: RenderEvent,
}

#[derive(Debug)]
enum SessionStreamingEvent {
    CreateSession {
        session_id: Uuid,
        provider_id: ProviderId,
        reply: oneshot::Sender<Result<(), SessionManagerError>>,
    },
    RestoreSession {
        session_id: Uuid,
    },
    RemoveSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<(), SessionManagerError>>,
    },
    AddEvent {
        session_id: Uuid,
        payload: SessionEvent,
    },
    ConstructMessages {
        session_id: Uuid,
        reply: oneshot::Sender<Result<Vec<Value>, SessionManagerError>>,
    },
    AvailableSessions {
        reply: oneshot::Sender<Result<Vec<(Uuid, ProviderId)>, SessionManagerError>>,
    },
}

#[derive(Debug)]
pub enum SessionEvent {
    UserPrompt(Value),
    Chat(ChatEvent),
    Err(String),
}

struct Session {
    provider_id: ProviderId,
    events: Vec<SessionEvent>, // single source of truth: prompts + deltas + OutputItems
    terminal: Option<Option<String>>,
}

pub struct SessionManager {
    sessions: HashMap<Uuid, Session>,
    event_rx: mpsc::Receiver<SessionStreamingEvent>,
    updates_tx: broadcast::Sender<SessionUpdate>,
}

#[derive(Clone)]
pub struct SessionManagerClient {
    event_tx: mpsc::Sender<SessionStreamingEvent>,
    updates_tx: broadcast::Sender<SessionUpdate>,
}

impl SessionManager {
    pub async fn new(
        session_path: PathBuf,
        storage: &Storage,
    ) -> Result<(Self, SessionManagerClient), StorageError> {
        let sessions: HashMap<Uuid, Session> = restore_sessions(session_path, storage).await?;

        let (tx, rx) = mpsc::channel(scry_config::SESSION_MANAGER_CHANNEL_CAPACITY);
        let (updates_tx, _) = broadcast::channel(scry_config::SESSION_BROADCAST_CHANNEL_CAPACITY);

        let manager = Self {
            sessions,
            event_rx: rx,
            updates_tx: updates_tx.clone(),
        };
        let client = SessionManagerClient {
            event_tx: tx,
            updates_tx,
        };

        Ok((manager, client))
    }

    pub async fn run(&mut self, session_writer_client: &SessionWriterClient) {
        while let Some(event) = self.event_rx.recv().await {
            if let Err(err) = self.handle_event(event, session_writer_client).await {
                error!("session manager error: {err}");
            }
        }
    }

    async fn handle_event(
        &mut self,
        event: SessionStreamingEvent,
        session_writer_client: &SessionWriterClient,
    ) -> scry_storage::Result<()> {
        match event {
            SessionStreamingEvent::CreateSession {
                session_id,
                provider_id,
                reply,
            } => {
                let result = match self.sessions.entry(session_id) {
                    Entry::Occupied(_) => {
                        Err(SessionManagerError::SessionAlreadyExists(session_id))
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(Session {
                            provider_id,
                            events: Vec::new(),
                            terminal: None,
                        });
                        Ok(())
                    }
                };
                let _ = reply.send(result);
            }
            SessionStreamingEvent::RestoreSession { session_id } => {
                if let Err(err) = self.restore_session(session_id) {
                    error!("session {session_id} restore_session failed: {err}");
                }
            }
            SessionStreamingEvent::RemoveSession { session_id, reply } => {
                if self.sessions.remove(&session_id).is_none() {
                    debug!("remove: session {session_id} not in memory");
                }
                let result = session_writer_client
                    .delete_file(session_id)
                    .await
                    .map_err(SessionManagerError::from);
                let _ = reply.send(result);
            }
            SessionStreamingEvent::AddEvent {
                session_id,
                payload,
            } => {
                if let Err(err) = self
                    .add_event(session_id, payload, session_writer_client)
                    .await
                {
                    error!("session {session_id} add_event failed: {err}");
                }
            }
            SessionStreamingEvent::ConstructMessages { session_id, reply } => {
                let _ = reply.send(self.construct_messages(session_id));
            }
            SessionStreamingEvent::AvailableSessions { reply } => {
                let sessions = self
                    .sessions
                    .iter()
                    .map(|(id, session)| (*id, session.provider_id))
                    .collect();
                let _ = reply.send(Ok(sessions));
            }
        }
        Ok(())
    }

    async fn add_event(
        &mut self,
        session_id: Uuid,
        payload: SessionEvent,
        session_writer_client: &SessionWriterClient,
    ) -> Result<(), SessionManagerError> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(SessionManagerError::UnknownSession(session_id));
        };

        let entry = match &payload {
            SessionEvent::UserPrompt(item)
            | SessionEvent::Chat(ChatEvent::OutputItem { item })
            | SessionEvent::Chat(ChatEvent::ToolCallItem { item }) => {
                Some((EntryType::ResponseItem, item.clone()))
            }
            SessionEvent::Chat(_) | SessionEvent::Err(_) => None,
        };

        let render_event = payload.to_render_event();
        session.update(payload);

        if let Some((t, item)) = entry {
            session_writer_client
                .append_file(
                    session_id,
                    FileEntry {
                        timestamp: chrono::Utc::now(),
                        t,
                        payload: item,
                    },
                )
                .await?;
        }

        if let Some(event) = render_event {
            let _ = self.updates_tx.send(SessionUpdate { session_id, event });
        }

        Ok(())
    }

    /// Restore session history from memory
    fn restore_session(&self, session_id: Uuid) -> Result<(), SessionManagerError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(SessionManagerError::UnknownSession(session_id))?;

        for event in &session.events {
            let render = match event {
                SessionEvent::Chat(ChatEvent::OutputItem { item }) => {
                    if item.get("type").and_then(|t| t.as_str()) != Some("message") {
                        continue;
                    }
                    let parts: Vec<String> = item
                        .get("content")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|c| {
                                    let kind = c.get("type").and_then(|t| t.as_str())?;
                                    match kind {
                                        "output_text" => {
                                            c.get("text").and_then(|x| x.as_str()).map(String::from)
                                        }
                                        "refusal" => c
                                            .get("refusal")
                                            .and_then(|x| x.as_str())
                                            .map(String::from),
                                        _ => None,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if parts.is_empty() {
                        error!("OutputItem (type=message) yielded no text from value: {item}");
                        continue;
                    }
                    RenderEvent::Chat(ChatRenderEvent::TextDelta {
                        text: parts.join("\n"),
                    })
                }
                SessionEvent::UserPrompt(_)
                | SessionEvent::Chat(ChatEvent::TextDelta { .. })
                | SessionEvent::Chat(ChatEvent::ReasoningSummaryDelta { .. }) => {
                    match event.to_render_event() {
                        Some(r) => r,
                        None => continue,
                    }
                }
                // All other cases are unexpected
                other => {
                    if other.to_render_event().is_some() {
                        error!(
                            "unexpected event in restored history for session {session_id}: {other:?}"
                        );
                    }
                    continue;
                }
            };

            let _ = self.updates_tx.send(SessionUpdate {
                session_id,
                event: render,
            });
        }
        Ok(())
    }

    fn construct_messages(&self, session_id: Uuid) -> Result<Vec<Value>, SessionManagerError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(SessionManagerError::UnknownSession(session_id))?;

        let messages = session
            .events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::UserPrompt(item)
                | SessionEvent::Chat(ChatEvent::OutputItem { item })
                | SessionEvent::Chat(ChatEvent::ToolCallItem { item }) => Some(item.clone()),
                SessionEvent::Chat(_) | SessionEvent::Err(_) => None,
            })
            .collect();
        Ok(messages)
    }
}

impl SessionEvent {
    fn to_render_event(&self) -> Option<RenderEvent> {
        match self {
            SessionEvent::Chat(ChatEvent::TextDelta { text }) => {
                Some(RenderEvent::Chat(ChatRenderEvent::TextDelta {
                    text: text.clone(),
                }))
            }
            SessionEvent::Chat(ChatEvent::ReasoningSummaryDelta { text }) => {
                Some(RenderEvent::Chat(ChatRenderEvent::ReasoningDelta {
                    text: text.clone(),
                }))
            }
            SessionEvent::Chat(ChatEvent::Done) => Some(RenderEvent::Done),
            SessionEvent::Err(message) => Some(RenderEvent::Error {
                // TODO need to check when it is the first prompt, but somehow error happens, should we keep the session and remove the prompt or we should remove the whole session
                message: message.clone(),
            }),
            SessionEvent::UserPrompt(item) => {
                let text = item
                    .get("content")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                if text.is_empty() {
                    error!("UserPrompt yielded no text from value: {item}");
                }
                Some(RenderEvent::Chat(ChatRenderEvent::UserPrompt { text }))
            }
            SessionEvent::Chat(ChatEvent::OutputItem { .. } | ChatEvent::ToolCallItem { .. }) => {
                None
            }
        }
    }
}

impl SessionManagerClient {
    pub async fn create_session(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
    ) -> Result<(), SessionManagerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::CreateSession {
                session_id,
                provider_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn add_event(
        &self,
        session_id: Uuid,
        payload: SessionEvent,
    ) -> Result<(), SessionManagerError> {
        self.event_tx
            .send(SessionStreamingEvent::AddEvent {
                session_id,
                payload,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)
    }

    pub async fn restore_session(&self, session_id: Uuid) -> Result<(), SessionManagerError> {
        self.event_tx
            .send(SessionStreamingEvent::RestoreSession { session_id })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)
    }

    pub async fn remove_session(&self, session_id: Uuid) -> Result<(), SessionManagerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::RemoveSession {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn construct_messages(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<Value>, SessionManagerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::ConstructMessages {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn available_sessions(&self) -> Result<Vec<(Uuid, ProviderId)>, SessionManagerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::AvailableSessions { reply: reply_tx })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    /// For UI process to subscribe for rendering update
    pub fn subscribe(&self) -> broadcast::Receiver<SessionUpdate> {
        self.updates_tx.subscribe()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionManagerError {
    #[error("unknown session {0}")]
    UnknownSession(Uuid),

    #[error("session {0} already exists")]
    SessionAlreadyExists(Uuid),

    #[error("session channel closed")]
    ChannelClosed,

    #[error("append failed: {0}")]
    Append(#[from] StorageError),
}

impl Session {
    fn update(&mut self, event: SessionEvent) {
        match event {
            event @ SessionEvent::Chat(
                ChatEvent::OutputItem { .. } | ChatEvent::ToolCallItem { .. },
            ) => {
                self.events.retain(|event| {
                    !matches!(
                        event,
                        SessionEvent::Chat(
                            ChatEvent::TextDelta { .. } | ChatEvent::ReasoningSummaryDelta { .. },
                        )
                    )
                });
                self.events.push(event);
            }
            SessionEvent::Chat(ChatEvent::Done) => {
                self.terminal = Some(None);
            }
            SessionEvent::Err(message) => self.terminal = Some(Some(message)),
            event @ (SessionEvent::UserPrompt(_)
            | SessionEvent::Chat(
                ChatEvent::TextDelta { .. } | ChatEvent::ReasoningSummaryDelta { .. },
            )) => {
                self.events.push(event);
            }
        }
    }
}

async fn restore_sessions(
    session_path: PathBuf,
    storage: &Storage,
) -> Result<HashMap<Uuid, Session>, StorageError> {
    let mut sessions: HashMap<Uuid, Session> = HashMap::new();

    for session in storage.all_sessions().await? {
        let id = match Uuid::parse_str(session.session_id.as_str()) {
            Ok(id) => id,
            Err(e) => {
                error!(
                    "skip session with invalid uuid {:?}: {e}",
                    session.session_id
                );
                continue;
            }
        };
        let provider_id = match session.provider_id.as_str() {
            "codex" => ProviderId::Codex,
            other => {
                error!("skip session {id} with unknown provider_id {other:?}");
                continue;
            }
        };

        let file_entries = match read_session_entries(&session_path, id).await {
            Ok(entries) => entries,
            Err(e) => {
                error!("skip session {id}: failed to read session file: {e}");
                continue;
            }
        };
        let events = file_entries
            .into_iter()
            .filter_map(|entry| match entry.t {
                EntryType::ResponseItem => Some(response_item_to_event(entry.payload)),
                EntryType::EventMsg => None,
            })
            .collect();

        sessions.insert(
            id,
            Session {
                provider_id,
                events,
                terminal: None,
            },
        );
    }

    Ok(sessions)
}

/// Reconstruct the in-memory `SessionEvent` for a persisted response item.
/// User messages come back as `UserPrompt`; everything else is an `OutputItem`.
fn response_item_to_event(payload: Value) -> SessionEvent {
    if payload.get("role").and_then(|r| r.as_str()) == Some("user") {
        SessionEvent::UserPrompt(payload)
    } else {
        SessionEvent::Chat(ChatEvent::OutputItem { item: payload })
    }
}
