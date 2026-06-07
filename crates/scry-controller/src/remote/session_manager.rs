use crate::entity::{ChatRenderEvent, RenderEvent};
use crate::remote::tool_controller::ToolCallPayload;
use crate::remote::PermissionWorkflowManagerClient;
use log::error;
use scry_capability::tools::shell::{Shell, ShellArgs};
use scry_capability::Tool;
use scry_provider::entity::{ChatEvent, ProviderId};
use scry_storage::StorageError;
use scry_storage::{EntryType, Storage};
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct SessionUpdate {
    pub session_id: Uuid,
    pub event: RenderEvent,
}

#[derive(Clone, Debug)]
pub struct SessionListItem {
    pub session_id: Uuid,
    pub provider_id: ProviderId,
    pub title: String,
}

#[derive(Debug)]
enum SessionStreamingEvent {
    CreateSession {
        session_id: Uuid,
        provider_id: ProviderId,
        title: String,
        reply: oneshot::Sender<Result<()>>,
    },
    RestoreSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<TerminalState>>,
    },
    RemoveSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    AddEvent {
        session_id: Uuid,
        payload: SessionEvent,
    },
    ConstructMessages {
        session_id: Uuid,
        reply: oneshot::Sender<Result<Vec<Value>>>,
    },
    AvailableSessions {
        reply: oneshot::Sender<Result<Vec<SessionListItem>>>,
    },
}

#[derive(Debug)]
pub enum SessionEvent {
    UserPrompt(Value),
    Chat(ChatEvent),
    Err(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalState {
    Running,
    Done,
    Error,
}

struct Session {
    /// only store delta, clear on DONE or ERROR
    delta: Vec<SessionEvent>,
    terminal: TerminalState,
}

pub struct SessionManager {
    sessions: HashMap<Uuid, Session>,
    event_rx: mpsc::Receiver<SessionStreamingEvent>,
    updates_tx: broadcast::Sender<SessionUpdate>,
    storage: Storage,
    permission_workflow_client: PermissionWorkflowManagerClient,
}

#[derive(Clone)]
pub struct SessionManagerClient {
    event_tx: mpsc::Sender<SessionStreamingEvent>,
    updates_tx: broadcast::Sender<SessionUpdate>,
}

impl SessionManager {
    pub async fn new(
        storage: Storage,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Result<(Self, SessionManagerClient)> {
        let sessions: HashMap<Uuid, Session> = restore_sessions(&storage).await?;

        let (tx, rx) = mpsc::channel(scry_config::SESSION_MANAGER_CHANNEL_CAPACITY);
        let (updates_tx, _) = broadcast::channel(scry_config::SESSION_BROADCAST_CHANNEL_CAPACITY);

        let manager = Self {
            sessions,
            event_rx: rx,
            updates_tx: updates_tx.clone(),
            storage,
            permission_workflow_client,
        };
        let client = SessionManagerClient {
            event_tx: tx,
            updates_tx,
        };

        Ok((manager, client))
    }

    pub async fn run(&mut self) {
        while let Some(event) = self.event_rx.recv().await {
            if let Err(err) = self.handle_event(event).await {
                error!("session manager error: {err}");
            }
        }
    }

    async fn handle_event(&mut self, event: SessionStreamingEvent) -> Result<()> {
        match event {
            SessionStreamingEvent::CreateSession {
                session_id,
                provider_id,
                title,
                reply,
            } => {
                let result = self
                    .storage
                    .create_new_session(session_id, provider_id.as_str(), &title)
                    .await
                    .map_err(SessionManagerError::from)
                    .and_then(|()| match self.sessions.entry(session_id) {
                        Entry::Occupied(_) => {
                            Err(SessionManagerError::SessionAlreadyExists(session_id))
                        }
                        Entry::Vacant(vacant) => {
                            vacant.insert(Session {
                                delta: Vec::new(),
                                terminal: TerminalState::Done,
                            });
                            Ok(())
                        }
                    });
                let _ = reply.send(result);
            }
            SessionStreamingEvent::RestoreSession { session_id, reply } => {
                let _ = reply.send(self.restore_session(session_id).await);
            }
            SessionStreamingEvent::RemoveSession { session_id, reply } => {
                let result = self
                    .storage
                    .delete_session(&session_id.to_string())
                    .await
                    .map(|()| {
                        self.sessions.remove(&session_id);
                    })
                    .map_err(SessionManagerError::from);
                let _ = reply.send(result);
            }
            SessionStreamingEvent::AddEvent {
                session_id,
                payload,
            } => {
                if let Err(err) = self.add_event(session_id, payload).await {
                    error!("session {session_id} add_event failed: {err}");
                }
            }
            SessionStreamingEvent::ConstructMessages { session_id, reply } => {
                let _ = reply.send(self.construct_messages(session_id).await);
            }
            SessionStreamingEvent::AvailableSessions { reply } => {
                let result = self
                    .storage
                    .all_sessions()
                    .await
                    .map(|sessions| {
                        sessions
                            .into_iter()
                            .filter_map(to_session_list_item)
                            .collect()
                    })
                    .map_err(SessionManagerError::from);
                let _ = reply.send(result);
            }
        }
        Ok(())
    }

    async fn add_event(&mut self, session_id: Uuid, payload: SessionEvent) -> Result<()> {
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

        let render_event = payload
            .to_render_event(&self.permission_workflow_client, session_id)
            .await;
        session.update(payload);

        // save only output item or tool call output item to db history
        if let Some((t, item)) = entry {
            self.storage
                .insert_history(&session_id.to_string(), t, &item)
                .await?;
        }

        if let Some(event) = render_event {
            let _ = self.updates_tx.send(SessionUpdate { session_id, event });
        }

        Ok(())
    }

    async fn restore_session(&self, session_id: Uuid) -> Result<TerminalState> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(SessionManagerError::UnknownSession(session_id))?;

        for entry in self
            .storage
            .restore_history(&session_id.to_string())
            .await?
        {
            let (event, finished) = match entry.t {
                EntryType::ResponseItem => (response_item_to_event(entry.payload), entry.finished),
                EntryType::EventMsg => continue,
            };

            let render = match &event {
                SessionEvent::Chat(ChatEvent::OutputItem { item }) => {
                    match item.get("type").and_then(|t| t.as_str()) {
                        Some("message") => {
                            let parts: Vec<String> = item
                                .get("content")
                                .and_then(|c| c.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|c| {
                                            let kind = c.get("type").and_then(|t| t.as_str())?;
                                            match kind {
                                                "output_text" => c
                                                    .get("text")
                                                    .and_then(|x| x.as_str())
                                                    .map(String::from),
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
                                error!(
                                    "OutputItem (type=message) yielded no text from value: {item}"
                                );
                                continue;
                            }
                            RenderEvent::Chat(ChatRenderEvent::TextDelta {
                                text: parts.join("\n"),
                            })
                        }
                        Some("web_search_call") => match web_search_render(item) {
                            Some(r) => r,
                            None => continue,
                        },
                        _ => continue,
                    }
                }
                SessionEvent::Chat(ChatEvent::ToolCallItem { item }) => {
                    match tool_call_render(
                        &self.permission_workflow_client,
                        session_id,
                        item,
                        finished,
                    )
                    .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                }
                SessionEvent::UserPrompt(_) => {
                    match event
                        .to_render_event(&self.permission_workflow_client, session_id)
                        .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                }
                other => {
                    error!(
                        "unexpected event in restored history for session {session_id}: {other:?}"
                    );
                    continue;
                }
            };

            let _ = self.updates_tx.send(SessionUpdate {
                session_id,
                event: render,
            });
        }

        // Replay in-flight streaming deltas not yet finalized into history, so a
        // re-subscribing UI catches up on the current turn's partial output.
        for event in &session.delta {
            if let Some(render) = event
                .to_render_event(&self.permission_workflow_client, session_id)
                .await
            {
                let _ = self.updates_tx.send(SessionUpdate {
                    session_id,
                    event: render,
                });
            }
        }

        Ok(session.terminal)
    }

    async fn construct_messages(&self, session_id: Uuid) -> Result<Vec<Value>> {
        let messages = self
            .storage
            .get_history(&session_id.to_string())
            .await?
            .into_iter()
            .filter_map(|entry| match entry.t {
                EntryType::ResponseItem => Some(response_item_to_event(entry.payload)),
                EntryType::EventMsg => None,
            })
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
    async fn to_render_event(
        &self,
        permission_workflow_manager_client: &PermissionWorkflowManagerClient,
        session_id: Uuid,
    ) -> Option<RenderEvent> {
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
                // should not render environment context in the UI
                if text.trim_start().starts_with("<environment_context>") {
                    return None;
                }
                if text.is_empty() {
                    error!("UserPrompt yielded no text from value: {item}");
                }
                Some(RenderEvent::Chat(ChatRenderEvent::UserPrompt { text }))
            }
            SessionEvent::Chat(ChatEvent::ToolCallItem { item }) => {
                // Live tool calls are always still pending when first rendered.
                tool_call_render(permission_workflow_manager_client, session_id, item, false).await
            }
            SessionEvent::Chat(ChatEvent::OutputItem { item }) => {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("web_search_call") => web_search_render(item),
                    _ => None,
                }
            }
        }
    }
}

/// TODO Currently only match for the web search call format from openAI, need to adapt other case later
fn web_search_render(item: &Value) -> Option<RenderEvent> {
    let action = item.get("action").cloned().unwrap_or(Value::Null);
    let arguments = serde_json::to_string_pretty(&action).unwrap_or_else(|_| action.to_string());
    Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
        name: "web_search".to_string(),
        arguments,
        description: None,
        decisions: vec![],
    }))
}

async fn tool_call_render(
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    session_id: Uuid,
    item: &Value,
    finished: bool,
) -> Option<RenderEvent> {
    match serde_json::from_value::<ToolCallPayload>(item.clone()) {
        Ok(call) => {
            // special handling for shell due to require user permission input
            if call.name == Shell::NAME {
                let args = match serde_json::from_str::<ShellArgs>(&call.arguments) {
                    Ok(args) => args,
                    Err(err) => {
                        error!("session {session_id}: malformed shell arguments: {err}");
                        return None;
                    }
                };
                // Do not check for permission if job is already finished
                let decisions = if finished {
                    vec![]
                } else {
                    match permission_workflow_manager_client
                        .check_decision(session_id, call.call_id)
                        .await
                    {
                        Ok(decisions) => decisions,
                        Err(err) => {
                            error!(
                                "session {session_id}: failed to check permission decision: {err}"
                            );
                            return None;
                        }
                    }
                };
                return Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
                    name: call.name,
                    arguments: args.command.join(" "),
                    description: Some(args.description),
                    decisions,
                }));
            }

            // Pretty print if it is json, otherwise, raw string
            let arguments = serde_json::from_str::<Value>(call.arguments.as_str())
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(call.arguments);

            Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
                name: call.name,
                arguments,
                description: None,
                decisions: vec![],
            }))
        }
        Err(err) => {
            error!("session {session_id}: malformed tool call: {err}");
            None
        }
    }
}

impl SessionManagerClient {
    pub async fn create_session(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
        title: String,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::CreateSession {
                session_id,
                provider_id,
                title,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn add_event(&self, session_id: Uuid, payload: SessionEvent) -> Result<()> {
        self.event_tx
            .send(SessionStreamingEvent::AddEvent {
                session_id,
                payload,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)
    }

    pub async fn restore_session(&self, session_id: Uuid) -> Result<TerminalState> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::RestoreSession {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn remove_session(&self, session_id: Uuid) -> Result<()> {
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

    pub async fn construct_messages(&self, session_id: Uuid) -> Result<Vec<Value>> {
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

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>> {
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

impl Session {
    fn update(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Chat(ChatEvent::OutputItem { .. } | ChatEvent::ToolCallItem { .. }) => {}
            SessionEvent::Chat(ChatEvent::Done) => {
                self.delta.clear();
                self.terminal = TerminalState::Done
            }
            SessionEvent::Err(_message) => {
                self.delta.clear();
                self.terminal = TerminalState::Error
            }
            SessionEvent::UserPrompt(_) => {
                // whenever new prompt coming, we consider the state as running.
                self.terminal = TerminalState::Running;
            }
            event @ SessionEvent::Chat(
                ChatEvent::TextDelta { .. } | ChatEvent::ReasoningSummaryDelta { .. },
            ) => {
                self.delta.push(event);
            }
        }
    }
}

async fn restore_sessions(storage: &Storage) -> Result<HashMap<Uuid, Session>> {
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

        sessions.insert(
            id,
            Session {
                delta: vec![],
                terminal: TerminalState::Done,
            },
        );
    }

    Ok(sessions)
}

fn to_session_list_item(session: scry_storage::Session) -> Option<SessionListItem> {
    let session_id = match Uuid::parse_str(&session.session_id) {
        Ok(id) => id,
        Err(e) => {
            error!(
                "skip session with invalid uuid {:?}: {e}",
                session.session_id
            );
            return None;
        }
    };
    let provider_id = match session.provider_id.as_str() {
        "codex" => ProviderId::Codex,
        other => {
            error!("skip session {session_id} with unknown provider_id {other:?}");
            return None;
        }
    };
    Some(SessionListItem {
        session_id,
        provider_id,
        title: session.title,
    })
}

fn response_item_to_event(payload: Value) -> SessionEvent {
    if payload.get("role").and_then(|r| r.as_str()) == Some("user") {
        SessionEvent::UserPrompt(payload)
    } else if payload.get("type").and_then(|t| t.as_str()) == Some("function_call") {
        SessionEvent::Chat(ChatEvent::ToolCallItem { item: payload })
    } else {
        SessionEvent::Chat(ChatEvent::OutputItem { item: payload })
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

    #[error(transparent)]
    Storage(#[from] StorageError),
}

type Result<T> = std::result::Result<T, SessionManagerError>;
