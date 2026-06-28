use futures::{Stream, stream};
use log::error;
use uuid::Uuid;

use crate::{
    RenderEvent,
    controller::{
        PermissionWorkflowError, ProviderControllerError, SessionManagerError,
        remote::{
            PermissionWorkflowManagerClient,
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
}

impl RemoteQuery {
    pub fn new(
        session_manager_client: SessionManagerClient,
        turn_manager_client: TurnManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Self {
        Self {
            session_manager_client,
            turn_manager_client,
            permission_workflow_client,
        }
    }

    pub async fn init_chat(
        &self,
        session_id: Option<Uuid>,
        prompt: String,
    ) -> Result<(Uuid, bool)> {
        match session_id {
            None => {
                let id = Uuid::now_v7();
                self.session_manager_client
                    .create_session(id, prompt)
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
        let mut rx = self
            .session_manager_client
            .subscribe(session_id, false)
            .await?;

        if let Err(error) = self
            .turn_manager_client
            .start_chat(session_id, provider_id, prompt)
            .await
        {
            return Err(error.into());
        }

        Ok(stream::poll_fn(move |cx| rx.poll_recv(cx)))
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
        let mut rx = self
            .session_manager_client
            .subscribe(session_id, true)
            .await?;

        if let Err(error) = self
            .session_manager_client
            .restore_session(session_id)
            .await
        {
            return Err(error.into());
        }

        Ok(stream::poll_fn(move |cx| rx.poll_recv(cx)))
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
