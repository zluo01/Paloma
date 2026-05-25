use crate::remote::session_manager::SessionManagerClient;
use crate::remote::SessionEvent;
use crate::{RuntimeController, RuntimeControllerError};
use futures::StreamExt;
use log::error;
use scry_provider::entity::{ChatEvent, ChatRequest, ProviderId};
use scry_storage::db::Storage;
use std::sync::Arc;
use uuid::Uuid;

const MAX_TITLE_CHARS: usize = 56;

pub struct RemoteQuery {
    runtime_controller: Arc<RuntimeController>,
    session_manager_client: SessionManagerClient,
    storage: Storage,
}

impl RemoteQuery {
    pub fn new(
        storage: Storage,
        runtime_controller: Arc<RuntimeController>,
        session_manager_client: SessionManagerClient,
    ) -> Self {
        Self {
            runtime_controller,
            session_manager_client,
            storage,
        }
    }

    /// First of the call chain, get or generate new session if it is new chat
    pub async fn init_chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<(Uuid, bool), RuntimeControllerError> {
        match session_id {
            None => {
                let id = Uuid::now_v7();
                self.storage
                    .create_new_session(id, provider_id.as_str(), &title_from_prompt(&prompt))
                    .await?;
                self.session_manager_client
                    .create_session(id, provider_id)
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
    ) -> Result<(), RuntimeControllerError> {
        let prefer_model_config = self
            .storage
            .prefer_model_config(provider_id.as_str())
            .await?;

        let client = self.runtime_controller.client(provider_id)?;

        let latest_prompt = client.construct_user_prompt(prompt);
        self.session_manager_client
            .add_event(session_id, SessionEvent::UserPrompt(latest_prompt))
            .await?;

        let messages = self
            .session_manager_client
            .construct_messages(session_id)
            .await?;

        let mut stream = client
            .chat(ChatRequest {
                model: prefer_model_config.model,
                effort: prefer_model_config.effort,
                messages,
            })
            .await?;

        let session_client = self.session_manager_client.clone();
        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                let session_event = match event {
                    Ok(chat_event) => SessionEvent::Chat(chat_event),
                    Err(err) => {
                        let message = err.to_string();
                        error!("chat stream error for session {session_id}: {message}");
                        SessionEvent::Err(message)
                    }
                };

                let is_terminal = matches!(
                    session_event,
                    SessionEvent::Chat(ChatEvent::Done) | SessionEvent::Err(_)
                );

                if let Err(err) = session_client.add_event(session_id, session_event).await {
                    error!("failed to insert event for session {session_id}: {err}");
                }

                if is_terminal {
                    break;
                }
            }
        });

        Ok(())
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

    pub async fn restore_session(&self, session_id: Uuid) {
        if let Err(err) = self
            .session_manager_client
            .restore_session(session_id)
            .await
        {
            error!("restore session {session_id}: {err}");
        }
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
