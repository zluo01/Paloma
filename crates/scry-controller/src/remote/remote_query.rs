use std::sync::Arc;

use log::error;
use scry_config::ENVIRONMENT_CONTEXT;
use scry_provider::ProviderId;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    remote::{
        permission_workflow_manager::{PermissionState, PermissionWorkflowError, UserDecision},
        session_manager::{
            SessionListItem, SessionManagerClient, SessionManagerError, TerminalState,
        },
        turn_manager::{TurnManagerClient, TurnManagerError},
        PermissionWorkflowManagerClient, SessionEvent, SessionUpdate,
    },
    ProviderController, ProviderControllerError,
};

const MAX_TITLE_CHARS: usize = 56;

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
                let title = title_from_prompt(&prompt);
                self.session_manager_client
                    .create_session(id, provider_id, title)
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
        self.turn_manager_client.cancel(session_id).await?;
        Ok(self.session_manager_client.cancel_event(session_id).await?)
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

fn title_from_prompt(prompt: &str) -> String {
    let s = prompt.lines().next().unwrap_or("").trim();
    if s.chars().count() <= MAX_TITLE_CHARS {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(MAX_TITLE_CHARS).collect();
        format!("{truncated}…")
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

pub type Result<T> = std::result::Result<T, RemoteQueryError>;
