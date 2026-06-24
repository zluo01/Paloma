use std::sync::Arc;

use futures::{Stream, stream};
use log::{debug, error};
use tokio::sync::{broadcast, broadcast::error::RecvError, mpsc};
use uuid::Uuid;

use crate::{
    RenderEvent,
    constants::{ENVIRONMENT_CONTEXT, RENDER_CHANNEL_CAPACITY},
    controller::{
        PermissionWorkflowError, ProviderController, ProviderControllerError, SessionManagerError,
        SessionUpdate,
        remote::{
            PermissionWorkflowManagerClient, SessionEvent,
            session_manager::{SessionListItem, SessionManagerClient},
            turn_manager::{TurnManagerClient, TurnManagerError},
        },
    },
    entity::ProviderId,
    permission::{PermissionState, UserDecision},
};

pub struct RemoteQuery {
    session_manager_client: SessionManagerClient,
    turn_manager_client: TurnManagerClient,
    permission_workflow_client: PermissionWorkflowManagerClient,
    provider_controller: Arc<ProviderController>,
}

impl RemoteQuery {
    pub fn new(
        session_manager_client: SessionManagerClient,
        turn_manager_client: TurnManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
        provider_controller: Arc<ProviderController>,
    ) -> Self {
        Self {
            session_manager_client,
            turn_manager_client,
            permission_workflow_client,
            provider_controller,
        }
    }

    pub async fn init_chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<(Uuid, bool)> {
        match session_id {
            None => {
                let id = Uuid::now_v7();
                self.session_manager_client
                    .create_session(id, provider_id, prompt)
                    .await?;

                // inject environment_context
                let client = self.provider_controller.client(provider_id)?;
                let env_prompt = client.construct_user_prompt(ENVIRONMENT_CONTEXT.clone());
                self.session_manager_client
                    .add_event(id, SessionEvent::UserPrompt(env_prompt))
                    .await?;

                Ok((id, true))
            },
            Some(id) => Ok((id, false)),
        }
    }

    /// new chat based on the session_id return from init_chat
    pub async fn chat(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<impl Stream<Item = RenderEvent> + use<>> {
        let rx = self.subscribe();
        let (render_tx, mut render_rx) = mpsc::channel(RENDER_CHANNEL_CAPACITY);

        let handle = tokio::spawn(forward_session_updates(rx, render_tx, session_id, "chat"));

        if let Err(error) = self
            .turn_manager_client
            .start_chat(session_id, provider_id, prompt)
            .await
        {
            handle.abort();
            return Err(error.into());
        }

        Ok(stream::poll_fn(move |cx| render_rx.poll_recv(cx)))
    }

    pub async fn cancel(&self, session_id: Uuid) -> Result<()> {
        // Only roll back when there was actually a running turn to abort.
        if self.turn_manager_client.cancel(session_id).await? {
            self.session_manager_client.cancel_event(session_id).await?;
        }
        Ok(())
    }

    // use for cleanup newly created session but the chat fails
    pub async fn cleanup(&self, session_id: Uuid) {
        if let Err(err) = self.session_manager_client.remove_session(session_id).await {
            error!("cleanup: remove session {session_id} from manager: {err}");
        }
    }

    pub async fn restore_session(
        &self,
        session_id: Uuid,
    ) -> Result<impl Stream<Item = RenderEvent> + use<>> {
        let rx = self.subscribe();
        let (render_tx, mut render_rx) = mpsc::channel(RENDER_CHANNEL_CAPACITY);

        let handle = tokio::spawn(forward_session_updates(
            rx, render_tx, session_id, "restore",
        ));

        if let Err(error) = self
            .session_manager_client
            .restore_session(session_id)
            .await
        {
            handle.abort();
            return Err(error.into());
        }

        Ok(stream::poll_fn(move |cx| render_rx.poll_recv(cx)))
    }

    pub async fn remove_session(&self, session_id: Uuid) -> Result<()> {
        // make sure we stop all active llm calls first.
        self.turn_manager_client.drop(session_id).await?;
        Ok(self
            .session_manager_client
            .remove_session(session_id)
            .await?)
    }

    pub async fn decide(&self, user_decision: UserDecision) -> Result<PermissionState> {
        Ok(self
            .permission_workflow_client
            .decide(user_decision)
            .await?)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionUpdate> {
        self.session_manager_client.subscribe()
    }

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>> {
        Ok(self.session_manager_client.available_sessions().await?)
    }
}

async fn forward_session_updates(
    mut rx: broadcast::Receiver<SessionUpdate>,
    render_tx: mpsc::Sender<RenderEvent>,
    session_id: Uuid,
    label: &'static str,
) {
    loop {
        match rx.recv().await {
            Ok(SessionUpdate {
                session_id: id,
                event,
            }) if id == session_id => {
                let done = matches!(
                    event,
                    RenderEvent::Done | RenderEvent::Error { .. } | RenderEvent::Cancel
                );
                if render_tx.send(event).await.is_err() {
                    debug!("{label} stream receiver dropped for session {session_id}");
                    break;
                }
                if done {
                    break;
                }
            },
            Ok(_) => {},
            Err(RecvError::Lagged(n)) => {
                error!("{label} stream lagged by {n} events for session {session_id}");
                break;
            },
            Err(RecvError::Closed) => break,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteQueryError {
    #[error(transparent)]
    ProviderController(#[from] ProviderControllerError),

    #[error(transparent)]
    TurnManager(#[from] TurnManagerError),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),

    #[error(transparent)]
    PermissionWorkflow(#[from] PermissionWorkflowError),
}

type Result<T> = std::result::Result<T, RemoteQueryError>;
