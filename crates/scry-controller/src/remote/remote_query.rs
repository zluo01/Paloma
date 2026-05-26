use crate::remote::session_manager::{SessionManagerClient, SessionManagerError, TerminalState};
use crate::remote::turn_manager::{TurnManagerClient, TurnManagerError};
use crate::remote::SessionEvent;
use crate::{ProviderController, ProviderControllerError};
use log::error;
use scry_config::ENVIRONMENT_CONTEXT;
use scry_provider::entity::ProviderId;
use scry_storage::db::Storage;
use std::sync::Arc;
use uuid::Uuid;

const MAX_TITLE_CHARS: usize = 56;

pub struct RemoteQuery {
    session_manager_client: SessionManagerClient,
    turn_manager_client: TurnManagerClient,
    provider_controller: Arc<ProviderController>,
    storage: Storage,
}

impl RemoteQuery {
    pub fn new(
        storage: Storage,
        session_manager_client: SessionManagerClient,
        turn_manager_client: TurnManagerClient,
        provider_controller: Arc<ProviderController>,
    ) -> Self {
        Self {
            session_manager_client,
            turn_manager_client,
            provider_controller,
            storage,
        }
    }

    pub async fn init_chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<(Uuid, bool), ProviderControllerError> {
        match session_id {
            None => {
                let id = Uuid::now_v7();
                let title = title_from_prompt(&prompt);
                self.storage
                    .create_new_session(id, provider_id.as_str(), &title)
                    .await?;
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
            }
            Some(id) => Ok((id, false)),
        }
    }

    /// new chat based on the session_id return from init_chat
    pub async fn chat(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<(), TurnManagerError> {
        self.turn_manager_client
            .start_chat(session_id, provider_id, prompt)
            .await
    }

    pub async fn cancel(&self, session_id: Uuid) -> Result<(), TurnManagerError> {
        self.turn_manager_client.cancel(session_id).await
    }

    // use for cleanup newly created session but the chat fails
    pub async fn cleanup(&self, session_id: Uuid) {
        if let Err(err) = self.session_manager_client.remove_session(session_id).await {
            error!("cleanup: remove session {session_id} from manager: {err}");
        }

        if let Err(err) = self.storage.delete_session(&session_id.to_string()).await {
            error!("cleanup: delete session {session_id} from storage: {err}");
        }
    }

    pub async fn restore_session(
        &self,
        session_id: Uuid,
    ) -> Result<TerminalState, SessionManagerError> {
        self.session_manager_client
            .restore_session(session_id)
            .await
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
