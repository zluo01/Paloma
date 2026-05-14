use crate::entity::{ChatRenderEvent, RenderEvent};
use crate::{RuntimeController, RuntimeControllerError};
use futures::StreamExt;
use log::error;
use scry_provider::entity::{ChatEvent, ChatRequest, ProviderId};
use scry_storage::db::Storage;
use scry_storage::session::{EntryType, FileEntry, WriterEvent};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct RemoteQuery {
    runtime_controller: Arc<RuntimeController>,
    writer_tx: mpsc::Sender<WriterEvent>,
    storage: Storage,
}

impl RemoteQuery {
    pub fn new(
        storage: Storage,
        runtime_controller: Arc<RuntimeController>,
        writer_tx: mpsc::Sender<WriterEvent>,
    ) -> Self {
        Self {
            runtime_controller,
            writer_tx,
            storage,
        }
    }

    pub async fn chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
        render_tx: mpsc::Sender<RenderEvent>,
    ) -> Result<Uuid, RuntimeControllerError> {
        let id = match session_id {
            None => {
                let id = Uuid::now_v7();
                self.storage
                    .create_new_session(&id, provider_id.as_str())
                    .await?;
                id
            }
            Some(id) => id,
        };

        let client = self.runtime_controller.client(provider_id)?;

        let latest_prompt = client.construct_user_prompt(prompt);
        self.writer_tx
            .send(WriterEvent::Append {
                session_id: id,
                entry: FileEntry {
                    timestamp: chrono::Utc::now(),
                    t: EntryType::EventMsg,
                    payload: latest_prompt.clone(),
                },
            })
            .await
            .map_err(|err| {
                error!("failed to persist user prompt for session {id}: {err}");
                RuntimeControllerError::WriterChannelClosed
            })?;

        let mut stream = client
            .chat(ChatRequest {
                model: "gpt-5.5".to_string(),
                effort: "medium".to_string(),
                messages: vec![latest_prompt],
            })
            .await?;

        let writer_tx = self.writer_tx.clone();
        let render_tx = render_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(ChatEvent::TextDelta { text }) => {
                        if let Err(err) = render_tx
                            .send(RenderEvent::Chat(ChatRenderEvent::TextDelta { text }))
                            .await
                        {
                            error!("failed to forward text delta for session {id}: {err}");
                        }
                    }
                    Ok(ChatEvent::ReasoningSummaryDelta { text }) => {
                        if let Err(err) = render_tx
                            .send(RenderEvent::Chat(ChatRenderEvent::ReasoningDelta { text }))
                            .await
                        {
                            error!("failed to forward reasoning delta for session {id}: {err}");
                        }
                    }
                    Ok(ChatEvent::OutputItem { item }) => {
                        if let Err(err) = writer_tx
                            .send(WriterEvent::Append {
                                session_id: id,
                                entry: FileEntry {
                                    timestamp: chrono::Utc::now(),
                                    t: EntryType::ResponseItem,
                                    payload: item,
                                },
                            })
                            .await
                        {
                            error!("failed to persist response item for session {id}: {err}");
                        }
                    }
                    Ok(ChatEvent::Done) => {
                        if let Err(err) = render_tx.send(RenderEvent::Done).await {
                            error!("failed to forward Done for session {id}: {err}");
                        }
                        break;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        error!("chat stream error for session {id}: {message}");
                        if let Err(send_err) = render_tx.send(RenderEvent::Error { message }).await
                        {
                            error!("failed to forward stream error for session {id}: {send_err}");
                        }
                        break;
                    }
                }
            }
        });

        Ok(id)
    }
}
