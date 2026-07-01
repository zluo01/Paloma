use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use log::error;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{
    constants::SESSION_MANAGER_CHANNEL_CAPACITY,
    controller::{
        ChatRenderEvent, PermissionWorkflowError, RenderEvent, ToolController,
        helper::{Disposition, extract_args},
        remote::PermissionWorkflowManagerClient,
    },
    db::{Session as StorageSession, Storage, StorageError},
    entity::ProviderId,
    provider::{ChatEvent, ConversationItem},
    utils::Gated,
};

#[derive(Clone, Debug)]
pub struct SessionListItem {
    pub session_id: Uuid,
    pub title: String,
    pub last_update: i64,
}

#[derive(Debug)]
enum SessionStreamingEvent {
    EnsureSession {
        session_id: Option<Uuid>,
        title: String,
        reply: oneshot::Sender<Result<(Uuid, bool)>>,
    },
    RestoreSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    RemoveSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    AddEvent {
        session_id: Uuid,
        provider_id: ProviderId,
        payload: SessionEvent,
        reply: oneshot::Sender<Result<()>>,
    },
    CancelEvent {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    AvailableSessions {
        reply: oneshot::Sender<Result<Vec<SessionListItem>>>,
    },
    Subscribe {
        session_id: Uuid,
        hold: bool,
        reply: oneshot::Sender<Result<mpsc::UnboundedReceiver<RenderEvent>>>,
    },
}

#[derive(Debug)]
pub enum SessionEvent {
    UserPrompt(String),
    Chat(ChatEvent),
    Err(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalState {
    Running,
    Done,
    Error,
    Cancel,
}

struct Session {
    /// only store delta, clear on DONE, ERROR or CANCEL
    delta: Vec<SessionEvent>,
    terminal: TerminalState,
    subscriber: Gated<mpsc::UnboundedSender<RenderEvent>>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            delta: vec![],
            terminal: TerminalState::Done,
            subscriber: Gated::empty(),
        }
    }
}

pub struct SessionManager {
    sessions: HashMap<Uuid, Session>,
    event_rx: mpsc::Receiver<SessionStreamingEvent>,
    storage: Storage,
    tool_controller: Arc<ToolController>,
    permission_workflow_client: PermissionWorkflowManagerClient,
}

#[derive(Clone)]
pub struct SessionManagerClient {
    event_tx: mpsc::Sender<SessionStreamingEvent>,
}

impl SessionManager {
    pub async fn new(
        storage: Storage,
        tool_controller: Arc<ToolController>,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Result<(Self, SessionManagerClient)> {
        let sessions: HashMap<Uuid, Session> = restore_sessions(&storage).await?;

        let (tx, rx) = mpsc::channel(SESSION_MANAGER_CHANNEL_CAPACITY);

        let manager = Self {
            sessions,
            event_rx: rx,
            storage,
            tool_controller,
            permission_workflow_client,
        };
        let client = SessionManagerClient { event_tx: tx };

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
            SessionStreamingEvent::EnsureSession {
                session_id,
                title,
                reply,
            } => {
                let _ = reply.send(self.ensure_session(session_id, title).await);
            },
            SessionStreamingEvent::RestoreSession { session_id, reply } => {
                let _ = reply.send(self.restore_session(session_id).await);
            },
            SessionStreamingEvent::RemoveSession { session_id, reply } => {
                let _ = reply.send(self.remove_session(session_id).await);
            },
            SessionStreamingEvent::AddEvent {
                session_id,
                provider_id,
                payload,
                reply,
            } => {
                let _ = reply.send(self.add_event(session_id, provider_id, payload).await);
            },
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
            },
            SessionStreamingEvent::CancelEvent { session_id, reply } => {
                let _ = reply.send(self.cancel_event(session_id).await);
            },
            SessionStreamingEvent::Subscribe {
                session_id,
                hold,
                reply,
            } => {
                let _ = reply.send(self.subscribe(session_id, hold));
            },
        }
        Ok(())
    }

    /// Resolve chat session:
    /// - Some + duplicate: existing session.
    /// - Some + insert: stale id reused as new session.
    /// - None: generated new session.
    async fn ensure_session(
        &mut self,
        session_id: Option<Uuid>,
        title: String,
    ) -> Result<(Uuid, bool)> {
        let id = session_id.unwrap_or_else(Uuid::now_v7);
        match self.storage.create_new_session(id, &title).await {
            Ok(_) => {
                self.sessions.insert(id, Session::default());
                Ok((id, true))
            },
            Err(StorageError::Duplicate(_)) => {
                match self.sessions.entry(id) {
                    Entry::Occupied(_) => {},
                    Entry::Vacant(vacant) => {
                        vacant.insert(Session::default());
                    },
                }
                Ok((id, false))
            },
            Err(error) => Err(error.into()),
        }
    }

    async fn add_event(
        &mut self,
        session_id: Uuid,
        provider_id: ProviderId,
        payload: SessionEvent,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(SessionManagerError::UnknownSession(session_id));
        };

        // should not trigger add event unless it is user prompt
        if session.terminal != TerminalState::Running
            && !matches!(&payload, SessionEvent::UserPrompt(_))
        {
            return Err(SessionManagerError::SessionNotRunning(session_id));
        }

        let entry = match &payload {
            SessionEvent::UserPrompt(prompt) => Some(ConversationItem::UserPrompt {
                prompt: prompt.clone(),
            }),
            SessionEvent::Chat(ChatEvent::OutputItem { item }) => Some(item.clone()),
            SessionEvent::Chat(_) | SessionEvent::Err(_) => None,
        };
        let errored = matches!(&payload, SessionEvent::Err(_));

        // Persist history items before publishing them to subscribers.
        if let Some(item) = entry {
            self.storage
                .insert_history(&session_id.to_string(), &provider_id, &item)
                .await?;
        }

        let render_event = payload
            .to_render_event(
                &self.permission_workflow_client,
                self.tool_controller.clone(),
                session_id,
            )
            .await;
        session.update(payload);

        if let Some(event) = render_event {
            let terminal = matches!(session.terminal, TerminalState::Done | TerminalState::Error);
            let delivered = if let Some(subscriber) = session.subscriber.get() {
                let _ = subscriber.send(event);
                true
            } else {
                false
            };
            if terminal && delivered {
                session.subscriber = Gated::empty();
            }
        }

        // a failed turn left partial items; roll the session back to its last
        // completed message so the next request starts from a valid state.
        if errored {
            self.rollback_session(session_id).await?;
        }

        Ok(())
    }

    async fn remove_session(&mut self, session_id: Uuid) -> Result<()> {
        self.storage.delete_session(&session_id.to_string()).await?;
        self.sessions.remove(&session_id);
        self.permission_workflow_client
            .remove_permission(session_id)
            .await?;
        Ok(())
    }

    async fn cancel_event(&mut self, session_id: Uuid) -> Result<()> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Err(SessionManagerError::UnknownSession(session_id));
        };

        // we should only able to cancel on running state
        if session.terminal == TerminalState::Running {
            session.terminal = TerminalState::Cancel;
            session.delta.clear();
            if let Some(subscriber) = session.subscriber.take() {
                let _ = subscriber.send(RenderEvent::Cancel);
            }
            self.rollback_session(session_id).await?;
        }
        Ok(())
    }

    /// rollback current session history due to error or cancel
    /// if it is the first prompt of the session triggered, we delete the session from db
    /// then cleanup the in-memory cache for session and permission.
    async fn rollback_session(&mut self, session_id: Uuid) -> Result<()> {
        self.storage
            .rollback_history(&session_id.to_string())
            .await?;
        if self
            .storage
            .delete_empty_session(&session_id.to_string())
            .await?
        {
            self.sessions.remove(&session_id);
            self.permission_workflow_client
                .remove_permission(session_id)
                .await?;
        } else {
            self.permission_workflow_client
                .clear_pending_permission(session_id)
                .await?
        }
        Ok(())
    }

    async fn restore_session(&mut self, session_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or(SessionManagerError::UnknownSession(session_id))?;

        session.subscriber.release();
        for entry in self
            .storage
            .restore_history(&session_id.to_string())
            .await?
        {
            let render = match entry.payload {
                ConversationItem::UserPrompt { prompt } => {
                    match SessionEvent::UserPrompt(prompt)
                        .to_render_event(
                            &self.permission_workflow_client,
                            self.tool_controller.clone(),
                            session_id,
                        )
                        .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                },
                ConversationItem::Message {
                    message,
                    provider_meta: _,
                } => {
                    let parts: Vec<String> = message.iter().map(|c| c.content.clone()).collect();
                    if parts.is_empty() {
                        error!(
                            "OutputItem (type=message) yielded no text from value: {:?}",
                            message
                        );
                        continue;
                    }
                    RenderEvent::Chat(ChatRenderEvent::TextDelta {
                        text: parts.join("\n"),
                        provider_id: entry.provider_id,
                    })
                },
                ConversationItem::ToolCall {
                    call_id,
                    name,
                    arguments,
                    provider_meta: _,
                } => {
                    match tool_call_render(
                        &self.permission_workflow_client,
                        self.tool_controller.clone(),
                        session_id,
                        &call_id,
                        &name,
                        &arguments,
                        entry.finished,
                    )
                    .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                },
                ConversationItem::HostedTool {
                    function_type,
                    content,
                    provider_meta: _,
                } => match hosted_tool_render(&function_type, content.as_deref().unwrap_or("")) {
                    None => continue,
                    Some(r) => r,
                },
                _ => continue,
            };

            if let Some(subscriber) = session.subscriber.get() {
                let _ = subscriber.send(render);
            }
        }

        match session.terminal {
            TerminalState::Running => {
                // Replay in-flight streaming deltas not yet finalized into history, so a
                // re-subscribing UI catches up on the current turn's partial output.
                for event in &session.delta {
                    if let Some(render) = event
                        .to_render_event(
                            &self.permission_workflow_client,
                            self.tool_controller.clone(),
                            session_id,
                        )
                        .await
                        && let Some(subscriber) = session.subscriber.get()
                    {
                        let _ = subscriber.send(render);
                    }
                }
            },
            TerminalState::Done | TerminalState::Error | TerminalState::Cancel => {
                if let Some(subscriber) = session.subscriber.take() {
                    let _ = subscriber.send(RenderEvent::Done);
                }
            },
        }

        Ok(())
    }

    fn subscribe(
        &mut self,
        session_id: Uuid,
        hold: bool,
    ) -> Result<mpsc::UnboundedReceiver<RenderEvent>> {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            let (sender, receiver) = mpsc::unbounded_channel::<RenderEvent>();
            session.subscriber = if hold {
                Gated::held(sender)
            } else {
                Gated::ready(sender)
            };
            return Ok(receiver);
        }
        Err(SessionManagerError::UnknownSession(session_id))
    }
}

impl SessionEvent {
    async fn to_render_event(
        &self,
        permission_workflow_manager_client: &PermissionWorkflowManagerClient,
        tool_controller: Arc<ToolController>,
        session_id: Uuid,
    ) -> Option<RenderEvent> {
        match self {
            SessionEvent::Chat(ChatEvent::TextDelta { provider_id, text }) => {
                Some(RenderEvent::Chat(ChatRenderEvent::TextDelta {
                    text: text.clone(),
                    provider_id: *provider_id,
                }))
            },
            SessionEvent::Chat(ChatEvent::ReasoningSummaryDelta { text }) => {
                Some(RenderEvent::Chat(ChatRenderEvent::ReasoningDelta {
                    text: text.clone(),
                }))
            },
            SessionEvent::Chat(ChatEvent::Done) => Some(RenderEvent::Done),
            SessionEvent::Err(message) => Some(RenderEvent::Error {
                message: message.clone(),
            }),
            SessionEvent::UserPrompt(prompt) => {
                Some(RenderEvent::Chat(ChatRenderEvent::UserPrompt {
                    text: prompt.clone(),
                }))
            },
            SessionEvent::Chat(ChatEvent::OutputItem { item }) => match item {
                ConversationItem::ToolCall {
                    call_id,
                    name,
                    arguments,
                    provider_meta: _,
                } => {
                    tool_call_render(
                        permission_workflow_manager_client,
                        tool_controller,
                        session_id,
                        call_id,
                        name,
                        arguments,
                        false,
                    )
                    .await
                },
                ConversationItem::HostedTool {
                    function_type,
                    content,
                    provider_meta: _,
                } => hosted_tool_render(function_type, content.as_deref().unwrap_or("")),
                _ => None,
            },
        }
    }
}

fn hosted_tool_render(function_type: &str, content: &str) -> Option<RenderEvent> {
    let arguments = serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| content.to_string());
    Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
        name: function_type.to_string(),
        arguments,
        description: None,
        decisions: vec![],
    }))
}

async fn tool_call_render(
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    tool_controller: Arc<ToolController>,
    session_id: Uuid,
    call_id: &str,
    name: &str,
    arguments: &str,
    finished: bool,
) -> Option<RenderEvent> {
    let disposition = tool_controller
        .retrieve_toolspec(name)
        .map(|spec| extract_args(spec, arguments))
        .unwrap_or(Disposition::Passthrough);

    let (arguments, description, decisions) = match disposition {
        Disposition::Gated(command, description) => {
            let decisions = if finished {
                vec![]
            } else {
                match permission_workflow_manager_client
                    .check_decision(session_id, call_id.to_string())
                    .await
                {
                    Ok(d) => d,
                    Err(err) => {
                        error!("session {session_id}: failed to check permission decision: {err}");
                        return None;
                    },
                }
            };
            (command.join(" "), description, decisions)
        },
        Disposition::Passthrough => {
            let args = serde_json::from_str::<Value>(arguments)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(arguments.to_string());
            (args, None, vec![])
        },
        Disposition::Skip => return None,
    };

    Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
        name: name.to_string(),
        arguments,
        description,
        decisions,
    }))
}

impl SessionManagerClient {
    pub async fn ensure_session(
        &self,
        session_id: Option<Uuid>,
        title: String,
    ) -> Result<(Uuid, bool)> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::EnsureSession {
                session_id,
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
        provider_id: ProviderId,
        payload: SessionEvent,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::AddEvent {
                session_id,
                provider_id,
                payload,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn cancel_event(&self, session_id: Uuid) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::CancelEvent {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| SessionManagerError::ChannelClosed)?
    }

    pub async fn restore_session(&self, session_id: Uuid) -> Result<()> {
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

    pub async fn subscribe(
        &self,
        session_id: Uuid,
        hold: bool,
    ) -> Result<mpsc::UnboundedReceiver<RenderEvent>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::Subscribe {
                session_id,
                hold,
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
}

impl Session {
    fn update(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Chat(ChatEvent::OutputItem { .. }) => {},
            SessionEvent::Chat(ChatEvent::Done) => {
                self.delta.clear();
                self.terminal = TerminalState::Done
            },
            SessionEvent::Err(_message) => {
                self.delta.clear();
                self.terminal = TerminalState::Error
            },
            SessionEvent::UserPrompt(_) => {
                // whenever new prompt coming, we consider the state as running.
                self.terminal = TerminalState::Running;
            },
            event @ SessionEvent::Chat(
                ChatEvent::TextDelta { .. } | ChatEvent::ReasoningSummaryDelta { .. },
            ) => {
                self.delta.push(event);
            },
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
            },
        };

        sessions.insert(
            id,
            Session {
                delta: vec![],
                terminal: TerminalState::Done,
                subscriber: Gated::empty(),
            },
        );
    }

    // cleanup history
    storage.recover_history().await?;

    Ok(sessions)
}

fn to_session_list_item(session: StorageSession) -> Option<SessionListItem> {
    let session_id = match Uuid::parse_str(&session.session_id) {
        Ok(id) => id,
        Err(e) => {
            error!(
                "skip session with invalid uuid {:?}: {e}",
                session.session_id
            );
            return None;
        },
    };
    Some(SessionListItem {
        session_id,
        title: session.title,
        last_update: session.last_update,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SessionManagerError {
    #[error("unknown session {0}")]
    UnknownSession(Uuid),

    #[error("session {0} already exists")]
    SessionAlreadyExists(Uuid),

    #[error("session {0} is not running. This indicates a bug.")]
    SessionNotRunning(Uuid),

    #[error("session channel closed")]
    ChannelClosed,

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    PermissionWorkflow(#[from] PermissionWorkflowError),
}

type Result<T> = std::result::Result<T, SessionManagerError>;
