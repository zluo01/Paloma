use std::sync::Arc;

use dashmap::DashMap;
use futures::StreamExt;
use log::{error, warn};
use paloma_provider_protocol::v1::{
    ChatRequest, ChatRequestMessage, ConversationItem, Done, ToolResult, UserPrompt,
    chat_response::Payload, conversation_item::Item,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use uuid::Uuid;

use super::helper::{Disposition, extract_args};
use crate::{
    constants::{INSTRUCTION, TURN_MANAGER_CHANNEL_CAPACITY},
    controller::{PermissionWorkflowManagerClient, ToolCallPayload, ToolController},
    db::{Storage, StorageError},
    entity::ProviderBackendId,
    provider::{ChatStream, ProviderController, ProviderControllerError},
    session::{SessionEvent, SessionManagerClient, SessionManagerError},
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
    Running(JoinHandle<()>, ProviderBackendId),
    Cancelled,
    Done,
}

#[derive(Debug)]
enum TurnStepEvent {
    /// for starting a new turn
    Start {
        session_id: Uuid,
        provider_backend_id: ProviderBackendId,
        prompt: String,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Self calling intermediate state, should never be called outside
    ToolCall {
        session_id: Uuid,
        provider_backend_id: ProviderBackendId,
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
                provider_backend_id,
                session_id,
                prompt,
                reply,
            } => {
                self.start_chat(provider_backend_id, session_id, prompt, reply)
                    .await;
            },
            TurnStepEvent::ToolCall {
                provider_backend_id,
                session_id,
                tool_calls,
            } => {
                self.tool_call(provider_backend_id, session_id, tool_calls)
                    .await;
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
        provider_backend_id: ProviderBackendId,
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
        let backend = provider_backend_id.clone();

        let handle = tokio::spawn(async move {
            let (messages, config) = match async {
                let config = storage.prefer_model_config(&provider_backend_id).await?;
                let messages = construct_messages(
                    &storage,
                    &session_client,
                    provider_backend_id.clone(),
                    session_id,
                    Some(prompt),
                )
                .await?;
                Ok::<_, TurnManagerError>((messages, config))
            }
            .await
            {
                Ok(messages) => {
                    let _ = reply.send(Ok(()));
                    messages
                },
                Err(error) => {
                    let _ = reply.send(Err(error));
                    let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
                    return;
                },
            };

            let tool_definitions = tools.iter().map(|t| t.to_definition()).collect();
            let stream = match provider_controller
                .chat(
                    provider_backend_id.clone(),
                    ChatRequest {
                        session_id: session_id.to_string(),
                        instruction: INSTRUCTION.into(),
                        model: config.model,
                        effort: config.effort,
                        messages,
                        tools: tool_definitions,
                    },
                )
                .await
            {
                Ok(stream) => stream,
                Err(error) => {
                    error!("fail to start chat. {}", error);
                    let _ = session_client
                        .add_event(
                            session_id,
                            provider_backend_id,
                            SessionEvent::Err(error.to_string()),
                        )
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
                provider_backend_id,
                session_id,
            )
            .await;
        });

        self.turn_map
            .insert(session_id, TurnState::Running(handle, backend));
    }

    async fn tool_call(
        &mut self,
        provider_backend_id: ProviderBackendId,
        session_id: Uuid,
        tool_calls: Vec<ToolCallPayload>,
    ) {
        // Continue only if the turn is not `Canceled`, `Done`, or a missing entry (session dropped/deleted)
        if !matches!(
            self.turn_map.get(&session_id).as_deref(),
            Some(TurnState::Running(..))
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
        let backend = provider_backend_id.clone();

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
                        provider_backend_id.clone(),
                        SessionEvent::Chat(Payload::OutputItem(ConversationItem {
                            item: Some(Item::ToolResult(ToolResult {
                                call_id,
                                name,
                                output,
                            })),
                        })),
                    )
                    .await
                {
                    error!("turn {session_id}: add tool output: {err}");
                }
            }

            let stream = match async {
                let config = storage.prefer_model_config(&provider_backend_id).await?;

                let messages = construct_messages(
                    &storage,
                    &session_client,
                    provider_backend_id.clone(),
                    session_id,
                    None,
                )
                .await?;

                let tool_definitions = tools.iter().map(|t| t.to_definition()).collect();
                let stream = provider_controller
                    .chat(
                        provider_backend_id.clone(),
                        ChatRequest {
                            session_id: session_id.to_string(),
                            instruction: INSTRUCTION.into(),
                            model: config.model,
                            effort: config.effort,
                            messages,
                            tools: tool_definitions,
                        },
                    )
                    .await?;

                Ok::<_, TurnManagerError>(stream)
            }
            .await
            {
                Ok(stream) => stream,
                Err(err) => {
                    error!("turn {session_id}: chat request failed during tool call turn: {err}");
                    let _ = session_client
                        .add_event(
                            session_id,
                            provider_backend_id.clone(),
                            SessionEvent::Err(err.to_string()),
                        )
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
                provider_backend_id,
                session_id,
            )
            .await;
        });

        self.turn_map
            .insert(session_id, TurnState::Running(handle, backend));
    }

    async fn abort_turn(&mut self, session_id: Uuid) -> Result<bool> {
        let provider_backend_id = {
            let Some(mut state) = self.turn_map.get_mut(&session_id) else {
                return Ok(false);
            };
            match std::mem::replace(&mut *state, TurnState::Cancelled) {
                TurnState::Running(handle, provider_backend_id) => {
                    handle.abort();
                    provider_backend_id
                },
                other => {
                    *state = other;
                    return Ok(false);
                },
            }
        };

        // best effort cancellation
        if let Err(err) = self
            .provider_controller
            .cancel_chat(provider_backend_id, session_id)
            .await
        {
            warn!("turn {session_id}: provider cancel_chat failed: {err}");
        }
        self.tool_controller.cancel_session(session_id).await;
        Ok(true)
    }

    async fn drop_turn(&mut self, session_id: Uuid) -> Result<()> {
        if let Some((_, TurnState::Running(handle, provider_backend_id))) =
            self.turn_map.remove(&session_id)
        {
            handle.abort();
            // best effort cancellation
            if let Err(err) = self
                .provider_controller
                .cancel_chat(provider_backend_id, session_id)
                .await
            {
                warn!("turn {session_id}: provider cancel_chat failed: {err}");
            }
            self.tool_controller.cancel_session(session_id).await;
        }
        Ok(())
    }

    fn mark_step_done(&self, session_id: Uuid) {
        if let Some(mut state) = self.turn_map.get_mut(&session_id)
            && matches!(*state, TurnState::Running(..))
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
    provider_backend_id: ProviderBackendId,
    session_id: Uuid,
) {
    let (tool_calls, errored) = exhaust_events(
        stream,
        session_client,
        tool_controller,
        permission_workflow_manager_client,
        session_id,
        provider_backend_id.clone(),
    )
    .await;

    // continue to run if there is tool calls and no error happens
    // otherwise, we should mark it as done
    if !errored && !tool_calls.is_empty() {
        let _ = event_tx
            .send(TurnStepEvent::ToolCall {
                session_id,
                provider_backend_id,
                tool_calls,
            })
            .await;
    } else {
        let _ = event_tx.send(TurnStepEvent::Done { session_id }).await;
    }
}

async fn construct_messages(
    storage: &Storage,
    session_client: &SessionManagerClient,
    provider_backend_id: ProviderBackendId,
    session_id: Uuid,
    prompt: Option<String>,
) -> Result<Vec<ChatRequestMessage>> {
    let mut messages: Vec<ChatRequestMessage> = storage
        .get_history(&session_id.to_string())
        .await?
        .into_iter()
        .map(|row| ChatRequestMessage {
            provider_id: row.provider_backend_id.provider_id,
            backend_id: row.provider_backend_id.backend_id,
            item: Some(row.payload),
        })
        .collect();

    if let Some(prompt) = prompt {
        session_client
            .add_event(
                session_id,
                provider_backend_id.clone(),
                SessionEvent::UserPrompt(prompt.clone()),
            )
            .await?;

        messages.push(ChatRequestMessage {
            provider_id: provider_backend_id.provider_id,
            backend_id: provider_backend_id.backend_id,
            item: Some(ConversationItem {
                item: Some(Item::UserPrompt(UserPrompt { prompt })),
            }),
        });
    }
    Ok(messages)
}

async fn exhaust_events(
    mut stream: ChatStream,
    session_client: &SessionManagerClient,
    tool_controller: Arc<ToolController>,
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    session_id: Uuid,
    provider_backend_id: ProviderBackendId,
) -> (Vec<ToolCallPayload>, bool) {
    let mut tool_calls: Vec<ToolCallPayload> = Vec::new();
    let mut errored = false;
    while let Some(event) = stream.next().await {
        let session_event = match event {
            Payload::Error(message) => {
                error!("chat stream error for session {session_id}: {message}");
                errored = true;
                SessionEvent::Err(message)
            },
            payload => {
                if let Payload::OutputItem(conversation_item) = &payload
                    && let Some(Item::ToolCall(tool_call)) = &conversation_item.item
                {
                    match tool_controller.retrieve_toolspec(&tool_call.name).await {
                        // it should be ok to only log error here since later on, when actual tool call happens
                        // it will still fail with missing call_id, session_id or missing tool name.
                        // Then we can populate the error back to the LLM.
                        Some(toolspec) => {
                            let command = match extract_args(toolspec, &tool_call.arguments) {
                                Disposition::Gated {
                                    require_permission, ..
                                } => Some(require_permission),
                                Disposition::Skip => Some(vec![]), // malform args, should mark as fail directly
                                Disposition::Passthrough => None,
                            };

                            if let Some(command) = command
                                && let Err(err) = permission_workflow_manager_client
                                    .init_permission_workflow(
                                        session_id,
                                        tool_call.call_id.clone(),
                                        command,
                                    )
                                    .await
                            {
                                error!(
                                    "session {session_id}: failed to init permission workflow: {err}"
                                );
                            }
                        },
                        None => {
                            error!("fail to find function call name {}", tool_call.name)
                        },
                    }
                    tool_calls.push(ToolCallPayload {
                        call_id: tool_call.call_id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                    });
                }
                SessionEvent::Chat(payload)
            },
        };

        // terminate on both error or done
        // if there is tool calls meaning current step is intermediate steps,
        // in this case, we should not signal the session_manager on turn finished
        let (is_terminal, forward) = match &session_event {
            SessionEvent::Err(_) => (true, true),
            SessionEvent::Chat(Payload::Done(Done {})) => (true, tool_calls.is_empty()),
            _ => (false, true),
        };

        if forward
            && let Err(err) = session_client
                .add_event(session_id, provider_backend_id.clone(), session_event)
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
        provider_backend_id: ProviderBackendId,
        prompt: String,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(TurnStepEvent::Start {
                session_id,
                provider_backend_id,
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
}

type Result<T> = std::result::Result<T, TurnManagerError>;
