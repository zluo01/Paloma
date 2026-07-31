use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use log::error;
use paloma_provider_protocol::v1::{
    ConversationItem, UserPrompt, chat_response, conversation_item::Item,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::helper::{Disposition, extract_args, prettify_arg};
use crate::{
    constants::SESSION_MANAGER_CHANNEL_CAPACITY,
    controller::{PermissionWorkflowError, PermissionWorkflowManagerClient, ToolController},
    db::{Session as StorageSession, Storage, StorageError},
    entity::{ChatRenderEvent, ProviderBackendId, RenderEvent},
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
        provider_backend_id: ProviderBackendId,
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
    Chat(chat_response::Payload),
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
                provider_backend_id,
                payload,
                reply,
            } => {
                let _ = reply.send(
                    self.add_event(session_id, provider_backend_id, payload)
                        .await,
                );
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
            Err(StorageError::DuplicateSession(_)) => {
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
        provider_backend_id: ProviderBackendId,
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
            SessionEvent::UserPrompt(prompt) => Some(ConversationItem {
                item: Some(Item::UserPrompt(UserPrompt {
                    prompt: prompt.clone(),
                })),
            }),
            SessionEvent::Chat(chat_response::Payload::OutputItem(item)) => Some(item.clone()),
            SessionEvent::Chat(_) | SessionEvent::Err(_) => None,
        };
        let errored = matches!(&payload, SessionEvent::Err(_));

        // Persist history items before publishing them to subscribers.
        if let Some(item) = entry {
            self.storage
                .insert_history(&session_id.to_string(), &provider_backend_id, &item)
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
        if self
            .storage
            .rollback_session_history(&session_id.to_string())
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
            let render = match entry.payload.item {
                Some(Item::UserPrompt(UserPrompt { prompt })) => {
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
                Some(Item::Message(message)) => {
                    if message.message.is_empty() {
                        error!(
                            "OutputItem (type=message) yielded no text from value: {:?}",
                            message
                        );
                        continue;
                    }
                    let parts: Vec<String> =
                        message.message.into_iter().map(|c| c.content).collect();
                    RenderEvent::Chat(ChatRenderEvent::TextDelta {
                        text: parts.join("\n"),
                        provider_backend_id: entry.provider_backend_id,
                    })
                },
                Some(Item::ToolCall(tool_call)) => {
                    match tool_call_render(
                        &self.permission_workflow_client,
                        self.tool_controller.clone(),
                        session_id,
                        &tool_call.call_id,
                        &tool_call.name,
                        &tool_call.arguments,
                        entry.finished,
                    )
                    .await
                    {
                        Some(r) => r,
                        None => continue,
                    }
                },
                Some(Item::HostedTool(hosted_tool)) => match hosted_tool_render(
                    &hosted_tool.function_type,
                    hosted_tool.content.as_deref().unwrap_or(""),
                ) {
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
            SessionEvent::Chat(chat_response::Payload::TextDelta(text_delta)) => {
                Some(RenderEvent::Chat(ChatRenderEvent::TextDelta {
                    text: text_delta.delta.clone(),
                    provider_backend_id: ProviderBackendId {
                        provider_id: text_delta.provider_id.clone(),
                        backend_id: text_delta.backend_id.clone(),
                    },
                }))
            },
            SessionEvent::Chat(chat_response::Payload::ReasoningDelta(text)) => {
                Some(RenderEvent::Chat(ChatRenderEvent::ReasoningDelta {
                    text: text.clone(),
                }))
            },
            SessionEvent::Chat(chat_response::Payload::Done(_)) => Some(RenderEvent::Done),
            SessionEvent::Err(message)
            | SessionEvent::Chat(chat_response::Payload::Error(message)) => {
                Some(RenderEvent::Error {
                    message: message.clone(),
                })
            },
            SessionEvent::UserPrompt(prompt) => {
                Some(RenderEvent::Chat(ChatRenderEvent::UserPrompt {
                    text: prompt.clone(),
                }))
            },
            SessionEvent::Chat(chat_response::Payload::OutputItem(item)) => match &item.item {
                Some(Item::ToolCall(tool_call)) => {
                    tool_call_render(
                        permission_workflow_manager_client,
                        tool_controller,
                        session_id,
                        &tool_call.call_id,
                        &tool_call.name,
                        &tool_call.arguments,
                        false,
                    )
                    .await
                },
                Some(Item::HostedTool(hosted_tool)) => hosted_tool_render(
                    &hosted_tool.function_type,
                    hosted_tool.content.as_deref().unwrap_or(""),
                ),
                _ => None,
            },
        }
    }
}

fn hosted_tool_render(function_type: &str, content: &str) -> Option<RenderEvent> {
    let arguments = prettify_arg(content);
    Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
        tool_name: function_type.to_string(),
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
    tool_id: &str,
    arguments: &str,
    finished: bool,
) -> Option<RenderEvent> {
    let disposition = tool_controller
        .retrieve_toolspec(tool_id)
        .await
        .map(|spec| extract_args(spec, arguments))
        .unwrap_or(Disposition::Passthrough);

    let (name, arguments, description, decisions) = match disposition {
        Disposition::Gated {
            name,
            description,
            arguments,
            ..
        } => {
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
            (name, arguments, description, decisions)
        },
        Disposition::Passthrough => {
            let args = prettify_arg(arguments);
            (tool_id.to_string(), args, None, vec![])
        },
        Disposition::Skip => return None,
    };

    Some(RenderEvent::Chat(ChatRenderEvent::ToolCall {
        tool_name: name,
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
        provider_backend_id: ProviderBackendId,
        payload: SessionEvent,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(SessionStreamingEvent::AddEvent {
                session_id,
                provider_backend_id,
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
            SessionEvent::Chat(chat_response::Payload::OutputItem(_)) => {},
            SessionEvent::Chat(chat_response::Payload::Done(_)) => {
                self.delta.clear();
                self.terminal = TerminalState::Done
            },
            SessionEvent::Err(_) | SessionEvent::Chat(chat_response::Payload::Error(_)) => {
                self.delta.clear();
                self.terminal = TerminalState::Error
            },
            SessionEvent::UserPrompt(_) => {
                // whenever new prompt coming, we consider the state as running.
                self.terminal = TerminalState::Running;
            },
            event @ SessionEvent::Chat(
                chat_response::Payload::TextDelta(_) | chat_response::Payload::ReasoningDelta(_),
            ) => {
                self.delta.push(event);
            },
        }
    }
}

async fn restore_sessions(storage: &Storage) -> Result<HashMap<Uuid, Session>> {
    // cleanup history first so emptied sessions never return by all_sessions
    storage.recover_history().await?;

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
