use std::sync::Arc;

use log::error;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    constants::ENVIRONMENT_CONTEXT,
    controller::{
        PermissionWorkflowError, ProviderController, ProviderControllerError, SessionManagerError,
        SessionUpdate, TerminalState,
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
    ) -> Result<()> {
        Ok(self
            .turn_manager_client
            .start_chat(session_id, provider_id, prompt)
            .await?)
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

    pub async fn restore_session(&self, session_id: Uuid) -> Result<TerminalState> {
        Ok(self
            .session_manager_client
            .restore_session(session_id)
            .await?)
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
