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
    config::TURN_MANAGER_CHANNEL_CAPACITY,
    controller::{
        ProviderController, ProviderControllerError, SessionManagerError, ToolController,
        helper::{Disposition, extract_args},
        remote::{
            PermissionWorkflowManagerClient, SessionEvent, session_manager::SessionManagerClient,
            tool_controller::ToolCallPayload,
        },
    },
    db::{ProviderId, Storage, StorageError},
    provider::{ChatEvent, ChatRequest, ChatStream, ProviderError},
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
                let _ = reply.send(Ok(self.abort_turn(session_id)));
            },
            TurnStepEvent::Drop { session_id, reply } => {
                self.drop_turn(session_id);
                let _ = reply.send(Ok(()));
            },
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
        let turn_map = self.turn_map.clone();
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
                Err(err) => {
                    let _ = reply.send(Err(err));
                    mark_step_done(&turn_map, session_id);
                    return;
                },
            };

            run_step(
                stream,
                &session_client,
                tool_controller,
                &permission_client,
                &event_tx,
                &turn_map,
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
        // Continue unless the turn was canceled.
        if matches!(
            self.turn_map.get(&session_id).as_deref(),
            Some(TurnState::Cancelled)
        ) {
            return;
        }

        let tool_controller = self.tool_controller.clone();
        let session_client = self.session_manager_client.clone();
        let permission_client = self.permission_workflow_client.clone();
        let provider_controller = self.provider_controller.clone();
        let storage = self.storage.clone();
        let event_tx = self.event_tx.clone();
        let turn_map = self.turn_map.clone();
        let tools = tool_controller.tool_schemas().await;

        let handle = tokio::spawn(async move {
            // Run all tool calls concurrently.
            let outputs = futures::future::join_all(tool_calls.iter().map(|call| {
                let tool_controller = &tool_controller;
                async move {
                    (
                        call.call_id.clone(),
                        tool_controller.exec(session_id, call).await,
                    )
                }
            }))
            .await;

            let client = match provider_controller.client(provider_id) {
                Ok(client) => client,
                Err(err) => {
                    error!("turn {session_id}: provider client: {err}");
                    mark_step_done(&turn_map, session_id);
                    return;
                },
            };

            for (call_id, output) in outputs {
                let item = client.construct_function_call_output(call_id, output);
                if let Err(err) = session_client
                    .add_event(
                        session_id,
                        SessionEvent::Chat(ChatEvent::OutputItem { item }),
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
                    error!("turn {session_id}: chat request failed: {err}");
                    let _ = session_client
                        .add_event(session_id, SessionEvent::Err(err.to_string()))
                        .await;
                    mark_step_done(&turn_map, session_id);
                    return;
                },
            };

            run_step(
                stream,
                &session_client,
                tool_controller,
                &permission_client,
                &event_tx,
                &turn_map,
                provider_id,
                session_id,
            )
            .await;
        });

        self.turn_map.insert(session_id, TurnState::Running(handle));
    }

    fn abort_turn(&mut self, session_id: Uuid) -> bool {
        let Some(mut state) = self.turn_map.get_mut(&session_id) else {
            return false;
        };
        match &*state {
            TurnState::Running(handle) => {
                handle.abort();
                *state = TurnState::Cancelled;
                true
            },
            TurnState::Cancelled | TurnState::Done => false,
        }
    }

    fn drop_turn(&mut self, session_id: Uuid) {
        if let Some((_, TurnState::Running(handle))) = self.turn_map.remove(&session_id) {
            handle.abort();
        }
    }
}

fn mark_step_done(turn_map: &DashMap<Uuid, TurnState>, session_id: Uuid) {
    if let Some(mut state) = turn_map.get_mut(&session_id)
        && matches!(*state, TurnState::Running(_))
    {
        *state = TurnState::Done;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_step(
    stream: ChatStream,
    session_client: &SessionManagerClient,
    tool_controller: Arc<ToolController>,
    permission_workflow_manager_client: &PermissionWorkflowManagerClient,
    event_tx: &mpsc::Sender<TurnStepEvent>,
    turn_map: &DashMap<Uuid, TurnState>,
    provider_id: ProviderId,
    session_id: Uuid,
) {
    let (tool_calls, errored) = exhaust_events(
        stream,
        session_client,
        tool_controller,
        permission_workflow_manager_client,
        session_id,
    )
    .await;

    mark_step_done(turn_map, session_id);

    if !errored && !tool_calls.is_empty() {
        let _ = event_tx
            .send(TurnStepEvent::ToolCall {
                session_id,
                provider_id,
                tool_calls,
            })
            .await;
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

    if let Some(prompt) = prompt {
        let user_prompt = client.construct_user_prompt(prompt);
        session_client
            .add_event(session_id, SessionEvent::UserPrompt(user_prompt))
            .await?;
    }

    let messages = session_client.construct_messages(session_id).await?;
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
) -> (Vec<ToolCallPayload>, bool) {
    let mut tool_calls: Vec<ToolCallPayload> = Vec::new();
    let mut errored = false;
    while let Some(event) = stream.next().await {
        let session_event = match event {
            Ok(chat_event) => {
                if let ChatEvent::ToolCallItem { item } = &chat_event {
                    match serde_json::from_value::<ToolCallPayload>(item.clone()) {
                        Ok(call) => {
                            match tool_controller.retrieve_toolspec(&call.name) {
                                // it should be ok to only log error here since later on, when actual tool call happens
                                // it will still fail with missing call_id, session_id or missing tool name.
                                // Then we can populate the error back to the LLM.
                                Some(toolspec) => {
                                    let command = match extract_args(toolspec, &call.arguments) {
                                        Disposition::Gated(command, _) => Some(command),
                                        Disposition::Skip => Some(vec![]), // malform args, should mark as fail directly
                                        Disposition::Passthrough => None,
                                    };

                                    if let Some(command) = command
                                        && let Err(err) = permission_workflow_manager_client
                                            .init_permission_workflow(
                                                session_id,
                                                call.call_id.clone(),
                                                command,
                                            )
                                            .await
                                    {
                                        error!(
                                            "session {session_id}: failed to init permission workflow: {err}"
                                        );
                                    }
                                    tool_calls.push(call);
                                },
                                None => error!("fail to find function call name {}", &call.name),
                            }
                        },
                        Err(err) => error!("session {session_id}: malformed tool call: {err}"),
                    }
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

        if forward && let Err(err) = session_client.add_event(session_id, session_event).await {
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
}

type Result<T> = std::result::Result<T, TurnManagerError>;
