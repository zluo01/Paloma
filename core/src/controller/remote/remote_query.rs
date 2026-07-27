use futures::{Stream, StreamExt, stream, stream::BoxStream};
use log::error;
use uuid::Uuid;

use crate::{
    ChatRenderEvent, RenderEvent,
    controller::{PermissionWorkflowError, remote::PermissionWorkflowManagerClient},
    entity::ProviderBackendId,
    permission::{PermissionState, UserDecision},
    provider::ProviderControllerError,
    session::{
        SessionListItem, SessionManagerClient, SessionManagerError, TurnManagerClient,
        TurnManagerError,
    },
};

pub struct RemoteQuery {
    session_manager_client: SessionManagerClient,
    turn_manager_client: TurnManagerClient,
    permission_workflow_client: PermissionWorkflowManagerClient,
}

pub struct ChatRenderStream {
    pub session_id: Option<Uuid>,
    pub stream: BoxStream<'static, RenderEvent>,
}

const CHAT_START_ERROR_MESSAGE: &str = "Internal Error. Fail to start a chat.";

#[derive(Debug)]
struct ChatStreamError {
    session_id: Option<Uuid>,
    is_new_session: bool,
    error: RemoteQueryError,
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

    pub async fn chat(
        &self,
        session_id: Option<Uuid>,
        provider_backend_id: ProviderBackendId,
        prompt: String,
    ) -> ChatRenderStream {
        match self
            .start_chat_stream(session_id, provider_backend_id, prompt.clone())
            .await
        {
            Ok(stream) => stream,
            Err(error) => self.chat_stream_error(&prompt, error).await,
        }
    }

    async fn start_chat_stream(
        &self,
        session_id: Option<Uuid>,
        provider_backend_id: ProviderBackendId,
        prompt: String,
    ) -> std::result::Result<ChatRenderStream, ChatStreamError> {
        let (session_id, is_new_session) = self
            .session_manager_client
            .ensure_session(session_id, prompt.clone())
            .await
            .map_err(|error| ChatStreamError {
                session_id,
                is_new_session: false,
                error: RemoteQueryError::from(error),
            })?;

        let mut rx = async {
            let rx = self
                .session_manager_client
                .subscribe(session_id, false)
                .await?;

            self.turn_manager_client
                .start_chat(session_id, provider_backend_id, prompt)
                .await?;

            Ok::<_, RemoteQueryError>(rx)
        }
        .await
        .map_err(|error| ChatStreamError {
            session_id: Some(session_id),
            is_new_session,
            error,
        })?;

        Ok(ChatRenderStream {
            session_id: Some(session_id),
            stream: stream::poll_fn(move |cx| rx.poll_recv(cx)).boxed(),
        })
    }

    async fn chat_stream_error(&self, prompt: &str, error: ChatStreamError) -> ChatRenderStream {
        let ChatStreamError {
            session_id,
            is_new_session,
            error,
        } = error;

        let mut latest_error = error;

        if is_new_session
            && let Some(session_id) = session_id
            && let Err(error) = self.session_manager_client.remove_session(session_id).await
        {
            latest_error = error.into();
        }

        error!("Fail to start chat. {}", latest_error);

        let returned_session_id = if is_new_session { None } else { session_id };

        ChatRenderStream {
            session_id: returned_session_id,
            stream: stream::iter([
                RenderEvent::Chat(ChatRenderEvent::UserPrompt {
                    text: prompt.to_string(),
                }),
                RenderEvent::Error {
                    message: CHAT_START_ERROR_MESSAGE.to_string(),
                },
            ])
            .boxed(),
        }
    }

    pub async fn cancel(&self, session_id: Uuid) -> Result<()> {
        // Only roll back when there was actually a running turn to abort.
        if self.turn_manager_client.cancel(session_id).await? {
            self.session_manager_client.cancel_event(session_id).await?;
        }
        Ok(())
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
