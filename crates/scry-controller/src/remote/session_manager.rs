use crate::entity::{ChatRenderEvent, RenderEvent};
use crate::remote::tool_controller::ToolCallPayload;
use crate::remote::PermissionWorkflowManagerClient;
use log::{debug, error};
use scry_capability::tools::shell::{Shell, ShellArgs};
use scry_capability::Tool;
use scry_provider::entity::{ChatEvent, ProviderId};
use scry_storage::db::Storage;
use scry_storage::session::{read_session_entries, EntryType, FileEntry, SessionWriterClient};
use scry_storage::StorageError;
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
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
        reply: oneshot::Sender<Result<(), SessionManagerError>>,
    },
    RestoreSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<TerminalState, SessionManagerError>>,
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
        reply: oneshot::Sender<Result<Vec<SessionListItem>, SessionManagerError>>,
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
    provider_id: ProviderId,
    title: String,
    events: Vec<SessionEvent>, // single source of truth: prompts + deltas + OutputItems
    terminal: TerminalState,
}

pub struct SessionManager {
    sessions: HashMap<Uuid, Session>,
    event_rx: mpsc::Receiver<SessionStreamingEvent>,
    updates_tx: broadcast::Sender<SessionUpdate>,
    permission_workflow_client: PermissionWorkflowManagerClient,
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
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Result<(Self, SessionManagerClient), StorageError> {
        let sessions: HashMap<Uuid, Session> = restore_sessions(session_path, storage).await?;

        let (tx, rx) = mpsc::channel(scry_config::SESSION_MANAGER_CHANNEL_CAPACITY);
        let (updates_tx, _) = broadcast::channel(scry_config::SESSION_BROADCAST_CHANNEL_CAPACITY);

        let manager = Self {
            sessions,
            event_rx: rx,
            updates_tx: updates_tx.clone(),
            permission_workflow_client,
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
                title,
                reply,
            } => {
                let result = match self.sessions.entry(session_id) {
                    Entry::Occupied(_) => {
                        Err(SessionManagerError::SessionAlreadyExists(session_id))
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(Session {
                            provider_id,
                            title,
                            events: Vec::new(),
                            terminal: TerminalState::Done,
                        });
                        Ok(())
                    }
                };
                let _ = reply.send(result);
            }
            SessionStreamingEvent::RestoreSession { session_id, reply } => {
                let _ = reply.send(self.restore_session(session_id).await);
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
                    .map(|(id, session)| SessionListItem {
                        session_id: *id,
                        provider_id: session.provider_id,
                        title: session.title.clone(),
                    })
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

        let render_event = payload
            .to_render_event(&self.permission_workflow_client, session_id)
            .await;
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

    async fn restore_session(
        &self,
        session_id: Uuid,
    ) -> Result<TerminalState, SessionManagerError> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(SessionManagerError::UnknownSession(session_id))?;

        let mut completed_tool_calls: HashSet<String> = HashSet::new();
        let mut renders: VecDeque<RenderEvent> = VecDeque::new();

        // walk the history backward to collect function_call_output
        // so we do not render unwanted actions for tool call permissions
        for event in session.events.iter().rev() {
            let render = match event {
                SessionEvent::Chat(ChatEvent::OutputItem { item }) => {
                    match item.get("type").and_then(|t| t.as_str()) {
                        Some("function_call_output") => {
                            if let Some(call_id) = item.get("call_id").and_then(|c| c.as_str()) {
                                completed_tool_calls.insert(call_id.to_string());
                            }
                            continue;
                        }
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
                    let finished = item
                        .get("call_id")
                        .and_then(|c| c.as_str())
                        .is_some_and(|call_id| completed_tool_calls.contains(call_id));
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
                SessionEvent::UserPrompt(_)
                | SessionEvent::Chat(ChatEvent::TextDelta { .. })
                | SessionEvent::Chat(ChatEvent::ReasoningSummaryDelta { .. }) => {
                    match event
                        .to_render_event(&self.permission_workflow_client, session_id)
                        .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                }
                // All other cases are unexpected
                other => {
                    if other
                        .to_render_event(&self.permission_workflow_client, session_id)
                        .await
                        .is_some()
                    {
                        error!(
                            "unexpected event in restored history for session {session_id}: {other:?}"
                        );
                    }
                    continue;
                }
            };

            renders.push_front(render);
        }

        for render in renders {
            let _ = self.updates_tx.send(SessionUpdate {
                session_id,
                event: render,
            });
        }

        Ok(session.terminal)
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
    ) -> Result<(), SessionManagerError> {
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

    pub async fn restore_session(
        &self,
        session_id: Uuid,
    ) -> Result<TerminalState, SessionManagerError> {
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

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>, SessionManagerError> {
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
                self.terminal = TerminalState::Done;
            }
            SessionEvent::Err(_message) => self.terminal = TerminalState::Error,
            event @ SessionEvent::UserPrompt(_) => {
                // whenever new prompt coming, we consider the state as running.
                self.terminal = TerminalState::Running;
                self.events.push(event);
            }
            event @ SessionEvent::Chat(
                ChatEvent::TextDelta { .. } | ChatEvent::ReasoningSummaryDelta { .. },
            ) => {
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
                title: session.title,
                events,
                terminal: TerminalState::Done,
            },
        );
    }

    Ok(sessions)
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
