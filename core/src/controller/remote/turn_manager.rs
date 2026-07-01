use std::sync::Arc;

use dashmap::DashMap;
use futures::StreamExt;
use log::error;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::{
    capability::ToolSchema,
    constants::TURN_MANAGER_CHANNEL_CAPACITY,
    controller::{
        ProviderController, ProviderControllerError, SessionManagerError, ToolController,
        ToolControllerError,
        helper::{Disposition, extract_args},
        remote::{
            PermissionWorkflowManagerClient, SessionEvent, session_manager::SessionManagerClient,
            tool_controller::ToolCallPayload,
        },
    },
    db::{HistoryEntry, Storage, StorageError},
    entity::ProviderId,
    provider::{ChatEvent, ChatRequest, ChatStream, ConversationItem, ProviderError},
};

pub struct TurnManager {
    turn_map: Arc<DashMap<Uuid, TurnState>>,
    provider_controller: Arc<ProviderController>,
    session_manager_client: SessionManagerClient,
    permission_workflow_client: PermissionWorkflowManagerClient,
    tool_controller: Arc<ToolController>,
    storage: Storage,
    event_rx: mpsc::Receiver<TurnStepEvent>,
    event_tx: mpsc::Sender<TurnStepEvent>,
}

#[derive(Clone)]
pub struct TurnManagerClient {
    event_tx: mpsc::Sender<TurnStepEvent>,
}

enum TurnState {
    Running(JoinHandle<()>),
    Cancelled,
    Done,
}

#[derive(Debug)]
enum TurnStepEvent {
    /// for starting a new turn
    Start {
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Self calling intermediate state, should never be called outside
    ToolCall {
        session_id: Uuid,
        provider_id: ProviderId,
        tool_calls: Vec<ToolCallPayload>,
    },
    /// cancelling the call
    Cancel {
        session_id: Uuid,
        reply: oneshot::Sender<Result<bool>>,
    },
    /// Drop the turn fully, cleanup when user wanna delete a session
    Drop {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Self calling end state, should never be called outside
    Done { session_id: Uuid },
}

impl TurnManager {
    pub fn new(
        storage: Storage,
        provider_controller: Arc<ProviderController>,
        session_manager_client: SessionManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
        tool_controller: Arc<ToolController>,
    ) -> (Self, TurnManagerClient) {
        let (tx, rx) = mpsc::channel(TURN_MANAGER_CHANNEL_CAPACITY);

        let manager = Self {
            turn_map: Arc::new(DashMap::new()),
            provider_controller,
            session_manager_client,
            permission_workflow_client,
            tool_controller,
            storage,
            event_rx: rx,
            event_tx: tx.clone(),
        };
        let client = TurnManagerClient { event_tx: tx };
        (manager, client)
    }

    pub async fn run(&mut self) {
        while let Some(event) = self.event_rx.recv().await {
            if let Err(err) = self.handle_event(event).await {
                error!("turn manager error: {err}");
            }
        }
    }

    async fn handle_event(&mut self, event: TurnStepEvent) -> Result<()> {
        match event {
            TurnStepEvent::Start {
                provider_id,
                session_id,
                prompt,
                reply,
            } => {
                self.start_chat(provider_id, session_id, prompt, reply)
                    .await;
            },
            TurnStepEvent::ToolCall {
                provider_id,
                session_id,
                tool_calls,
            } => {
                self.tool_call(provider_id, session_id, tool_calls).await;
            },
            TurnStepEvent::Cancel { session_id, reply } => {
                let _ = reply.send(self.abort_turn(session_id).await);
            },
            TurnStepEvent::Drop { session_id, reply } => {
                let _ = reply.send(self.drop_turn(session_id).await);
            },
            TurnStepEvent::Done { session_id } => self.mark_step_done(session_id),
        }
        Ok(())
    }

    async fn start_chat(
        &mut self,
        provider_id: ProviderId,
        session_id: Uuid,
        prompt: String,
        reply: oneshot::Sender<Result<()>>,
    ) {
        let provider_controller = self.provider_controller.clone();
        let tool_controller = self.tool_controller.clone();
        let storage = self.storage.clone();
        let session_client = self.session_manager_client.clone();
        let permission_client = self.permission_workflow_client.clone();
        let event_tx = self.event_tx.clone();
        let tools = self.tool_controller.tool_schemas().await;

        let handle = tokio::spawn(async move {
            let stream = match open_stream(
                provider_controller,
                storage,
                session_client.clone(),
                provider_id,
                session_id,
                Some(prompt),
                tools,
            )
            .await
            {
                Ok(stream) => {
                    let _ = reply.send(Ok(()));
                    stream
                },
                Err(TurnManagerError::Provider(error)) => {
                    let result = session_client
                        .add_event(
                            session_id,
                            provider_id,
                            SessionEvent::Err(error.to_string()),
                        )
                        .await
                        .map_err(TurnManagerError::from);

                    let _ = reply.send(result);
                    let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
                    return;
                },
                Err(err) => {
                    let _ = reply.send(Err(err));
                    let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
                    return;
                },
            };

            run_step(
                stream,
                &session_client,
                tool_controller,
                &permission_client,
                &event_tx,
                provider_id,
                session_id,
            )
            .await;
        });

        self.turn_map.insert(session_id, TurnState::Running(handle));
    }

    async fn tool_call(
        &mut self,
        provider_id: ProviderId,
        session_id: Uuid,
        tool_calls: Vec<ToolCallPayload>,
    ) {
        // Continue only if the turn is not `Canceled`, `Done`, or a missing entry (session dropped/deleted)
        if !matches!(
            self.turn_map.get(&session_id).as_deref(),
            Some(TurnState::Running(_))
        ) {
            return;
        }

        let tool_controller = self.tool_controller.clone();
        let session_client = self.session_manager_client.clone();
        let permission_client = self.permission_workflow_client.clone();
        let provider_controller = self.provider_controller.clone();
        let storage = self.storage.clone();
        let event_tx = self.event_tx.clone();
        let tools = tool_controller.tool_schemas().await;

        let handle = tokio::spawn(async move {
            // Run all tool calls concurrently.
            let outputs = futures::future::join_all(tool_calls.iter().map(|call| {
                let tool_controller = &tool_controller;
                async move {
                    (
                        call.call_id.clone(),
                        call.name.clone(),
                        tool_controller.exec(session_id, call).await,
                    )
                }
            }))
            .await;

            for (call_id, name, output) in outputs {
                if let Err(err) = session_client
                    .add_event(
                        session_id,
                        provider_id,
                        SessionEvent::Chat(ChatEvent::OutputItem {
                            item: ConversationItem::ToolResult {
                                call_id,
                                name,
                                output,
                            },
                        }),
                    )
                    .await
                {
                    error!("turn {session_id}: add tool output: {err}");
                }
            }

            let stream = match open_stream(
                provider_controller,
                storage,
                session_client.clone(),
                provider_id,
                session_id,
                None,
                tools,
            )
            .await
            {
                Ok(stream) => stream,
                Err(err) => {
                    error!("turn {session_id}: chat request failed during tool call turn: {err}");
                    let _ = session_client
                        .add_event(session_id, provider_id, SessionEvent::Err(err.to_string()))
                        .await;
                    let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
                    return;
                },
            };

            run_step(
                stream,
                &session_client,
                tool_controller,
                &permission_client,
                &event_tx,
                provider_id,
                session_id,
            )
            .await;
        });

        self.turn_map.insert(session_id, TurnState::Running(handle));
    }

    async fn abort_turn(&mut self, session_id: Uuid) -> Result<bool> {
        {
            let Some(mut state) = self.turn_map.get_mut(&session_id) else {
                return Ok(false);
            };
            match &*state {
                TurnState::Running(handle) => {
                    handle.abort();
                    *state = TurnState::Cancelled;
                },
                TurnState::Cancelled | TurnState::Done => return Ok(false),
            }
        }

        self.tool_controller.cancel_session(session_id).await?;
        Ok(true)
    }

    async fn drop_turn(&mut self, session_id: Uuid) -> Result<()> {
        if let Some((_, TurnState::Running(handle))) = self.turn_map.remove(&session_id) {
            handle.abort();
            self.tool_controller.cancel_session(session_id).await?;
        }
        Ok(())
    }

    fn mark_step_done(&self, session_id: Uuid) {
        if let Some(mut state) = self.turn_map.get_mut(&session_id)
            && matches!(*state, TurnState::Running(_))
        {
            *state = TurnState::Done;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_step(
    stream: ChatStream,
    session_client: &SessionManagerClient,
    tool_controller: Arc<ToolController>,
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    event_tx: &mpsc::Sender<TurnStepEvent>,
    provider_id: ProviderId,
    session_id: Uuid,
) {
    let (tool_calls, errored) = exhaust_events(
        stream,
        session_client,
        tool_controller,
        permission_workflow_manager_client,
        session_id,
        provider_id,
    )
    .await;

    // continue to run if there is tool calls and no error happens
    // otherwise, we should mark it as done
    if !errored && !tool_calls.is_empty() {
        let _ = event_tx
            .send(TurnStepEvent::ToolCall {
                session_id,
                provider_id,
                tool_calls,
            })
            .await;
    } else {
        let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
    }
}

async fn open_stream(
    provider_controller: Arc<ProviderController>,
    storage: Storage,
    session_client: SessionManagerClient,
    provider_id: ProviderId,
    session_id: Uuid,
    prompt: Option<String>,
    tools: Vec<ToolSchema>,
) -> Result<ChatStream> {
    let client = provider_controller.client(provider_id)?;
    let config = storage.prefer_model_config(&provider_id).await?;

    let mut messages = storage.get_history(&session_id.to_string()).await?;

    if let Some(prompt) = prompt {
        session_client
            .add_event(
                session_id,
                provider_id,
                SessionEvent::UserPrompt(prompt.clone()),
            )
            .await?;

        messages.push(HistoryEntry {
            provider_id,
            payload: ConversationItem::UserPrompt { prompt },
        });
    }

    let stream = client
        .chat(ChatRequest {
            model: config.model,
            effort: config.effort,
            messages,
            tools,
        })
        .await?;
    Ok(stream)
}

async fn exhaust_events(
    mut stream: ChatStream,
    session_client: &SessionManagerClient,
    tool_controller: Arc<ToolController>,
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    session_id: Uuid,
    provider_id: ProviderId,
) -> (Vec<ToolCallPayload>, bool) {
    let mut tool_calls: Vec<ToolCallPayload> = Vec::new();
    let mut errored = false;
    while let Some(event) = stream.next().await {
        let session_event = match event {
            Ok(chat_event) => {
                if let ChatEvent::OutputItem {
                    item:
                        ConversationItem::ToolCall {
                            call_id,
                            name,
                            arguments,
                            provider_meta: _,
                        },
                } = &chat_event
                {
                    match tool_controller.retrieve_toolspec(name) {
                        // it should be ok to only log error here since later on, when actual tool call happens
                        // it will still fail with missing call_id, session_id or missing tool name.
                        // Then we can populate the error back to the LLM.
                        Some(toolspec) => {
                            let command = match extract_args(toolspec, arguments) {
                                Disposition::Gated(command, _) => Some(command),
                                Disposition::Skip => Some(vec![]), // malform args, should mark as fail directly
                                Disposition::Passthrough => None,
                            };

                            if let Some(command) = command
                                && let Err(err) = permission_workflow_manager_client
                                    .init_permission_workflow(session_id, call_id.clone(), command)
                                    .await
                            {
                                error!(
                                    "session {session_id}: failed to init permission workflow: {err}"
                                );
                            }
                        },
                        None => {
                            error!("fail to find function call name {}", name)
                        },
                    }
                    tool_calls.push(ToolCallPayload {
                        call_id: call_id.to_string(),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    });
                }
                SessionEvent::Chat(chat_event)
            },
            Err(err) => {
                let message = err.to_string();
                error!("chat stream error for session {session_id}: {message}");
                errored = true;
                SessionEvent::Err(message)
            },
        };

        // terminate on both error or done
        // if there is tool calls meaning current step is intermediate steps,
        // in this case, we should not signal the session_manager on turn finished
        let (is_terminal, forward) = match &session_event {
            SessionEvent::Err(_) => (true, true),
            SessionEvent::Chat(ChatEvent::Done) => (true, tool_calls.is_empty()),
            _ => (false, true),
        };

        if forward
            && let Err(err) = session_client
                .add_event(session_id, provider_id, session_event)
                .await
        {
            error!("failed to insert event for session {session_id}: {err}");
        }

        if is_terminal {
            break;
        }
    }
    (tool_calls, errored)
}

impl TurnManagerClient {
    pub async fn start_chat(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(TurnStepEvent::Start {
                session_id,
                provider_id,
                prompt,
                reply: reply_tx,
            })
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?
    }

    pub async fn cancel(&self, session_id: Uuid) -> Result<bool> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(TurnStepEvent::Cancel {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?
    }

    pub async fn drop(&self, session_id: Uuid) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(TurnStepEvent::Drop {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| TurnManagerError::ChannelClosed)?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TurnManagerError {
    #[error("turn manager channel closed")]
    ChannelClosed,

    #[error(transparent)]
    Runtime(#[from] ProviderControllerError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Session(#[from] SessionManagerError),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    ToolController(#[from] ToolControllerError),
}

type Result<T> = std::result::Result<T, TurnManagerError>;
