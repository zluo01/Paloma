use std::collections::HashSet;
use std::sync::Arc;

use crate::{RuntimeController, RuntimeControllerError};
use dashmap::DashMap;
use scry_provider::connector::CodexConnector;
use scry_provider::entity::{Auth, Connection, ProviderAuthenticator, ProviderError, ProviderId};
use scry_storage::storage::Storage;
use scry_storage::StorageError;
use serde::{Deserialize, Serialize};

pub struct ConnectController {
    handlers: DashMap<ProviderId, Arc<dyn ProviderAuthenticator>>,
    runtime_controller: RuntimeController,
    storage: Storage,
}

impl ConnectController {
    pub fn new(
        storage: Storage,
        runtime_controller: RuntimeController,
        http: reqwest::Client,
    ) -> Self {
        let handlers: DashMap<ProviderId, Arc<dyn ProviderAuthenticator>> = DashMap::new();

        let codex: Arc<dyn ProviderAuthenticator> = Arc::new(CodexConnector::new(http));
        handlers.insert(codex.id(), codex);

        Self {
            handlers,
            runtime_controller,
            storage,
        }
    }

    pub async fn init(&self, provider_id: ProviderId) -> Result<Connection, ConnectError> {
        let handler = self.handler(provider_id)?;
        Ok(handler.init_connection().await?)
    }

    pub async fn finalize(
        &self,
        provider_id: ProviderId,
        payload: Connection,
    ) -> Result<Auth, ConnectError> {
        let handler = self.handler(provider_id)?;
        let auth = handler.finalize_connection(payload).await?;

        self.runtime_controller
            .new_model(provider_id, &auth)
            .await?;

        let models = self.runtime_controller.models(provider_id).await?;
        let default = models
            .first()
            .ok_or(ConnectError::NoModelsAvailable(provider_id))?;

        // Persist credentials. On failure, roll back the in-memory
        // registration so the daemon doesn't carry a ghost runtime that
        // won't survive a restart.
        if let Err(e) = self
            .persist(
                provider_id,
                &auth,
                &default.id,
                &default.default_reasoning_effort,
            )
            .await
        {
            self.runtime_controller.remove_model(provider_id);
            return Err(e);
        }

        Ok(auth)
    }

    pub async fn disconnect(&self, provider_id: ProviderId) -> Result<(), ConnectError> {
        self.storage.delete_provider(provider_id.as_str()).await?;
        self.runtime_controller.remove_model(provider_id);
        Ok(())
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>, ConnectError> {
        let connected: HashSet<String> = self
            .storage
            .connected_providers()
            .await?
            .into_iter()
            .map(|p| p.provider_id)
            .collect();

        let mut ids: Vec<ProviderId> = self.handlers.iter().map(|entry| *entry.key()).collect();
        ids.sort_by_key(|id| id.as_str());

        Ok(ids
            .into_iter()
            .map(|id| Connector {
                connected: connected.contains(id.as_str()),
                id,
            })
            .collect())
    }

    fn handler(
        &self,
        provider_id: ProviderId,
    ) -> Result<Arc<dyn ProviderAuthenticator>, ConnectError> {
        self.handlers
            .get(&provider_id)
            .map(|h| Arc::clone(h.value()))
            .ok_or(ConnectError::UnknownProvider(provider_id))
    }

    async fn persist(
        &self,
        provider_id: ProviderId,
        auth: &Auth,
        prefer_model: &str,
        prefer_effort: &str,
    ) -> Result<(), ConnectError> {
        let kind = auth.kind_str();
        match auth {
            Auth::ApiKey(secret) => {
                self.storage
                    .insert_provider(
                        provider_id.as_str(),
                        kind,
                        secret,
                        None,
                        prefer_model,
                        prefer_effort,
                    )
                    .await?;
            }
            Auth::OAuth {
                refresh_token,
                expires_at_unix,
            } => {
                let secret = refresh_token
                    .as_deref()
                    .ok_or(ConnectError::MissingRefreshToken)?;
                self.storage
                    .insert_provider(
                        provider_id.as_str(),
                        kind,
                        secret,
                        *expires_at_unix,
                        prefer_model,
                        prefer_effort,
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    id: ProviderId,
    connected: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("unknown provider: {0:?}")]
    UnknownProvider(ProviderId),

    #[error("OAuth response did not include a refresh_token")]
    MissingRefreshToken,

    #[error("provider {0:?} returned no models")]
    NoModelsAvailable(ProviderId),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Runtime(#[from] RuntimeControllerError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}
