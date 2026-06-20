use std::{collections::HashMap, sync::Arc};

use dashmap::DashMap;
use log::error;

use crate::{
    controller::{ProviderController, remote::ProviderStatis},
    db::{AuthKind, ConnectedProvider, Storage, StorageError},
    entity::{HealthLevel, HealthStatus, ProviderId},
    provider::{Auth, CodexConnector, Connection, ProviderAuthenticator, ProviderError},
};

pub struct ConnectController {
    handlers: DashMap<ProviderId, Arc<dyn ProviderAuthenticator>>,
    provider_controller: Arc<ProviderController>,
    storage: Storage,
}

impl ConnectController {
    pub fn new(
        storage: Storage,
        provider_controller: Arc<ProviderController>,
        http: reqwest::Client,
    ) -> Self {
        let handlers: DashMap<ProviderId, Arc<dyn ProviderAuthenticator>> = DashMap::new();

        let codex: Arc<dyn ProviderAuthenticator> = Arc::new(CodexConnector::new(http));
        handlers.insert(codex.id(), codex);

        Self {
            handlers,
            provider_controller,
            storage,
        }
    }

    pub async fn init(&self, provider_id: ProviderId) -> Result<Connection> {
        let handler = self.handler(provider_id)?;
        Ok(handler.init_connection().await?)
    }

    pub async fn finalize(&self, provider_id: ProviderId, payload: Connection) -> Result<()> {
        let handler = self.handler(provider_id)?;
        let auth = handler.finalize_connection(payload).await?;

        // init to db first so during initialization, we have target to update for tokens.
        let (kind, secret) = match &auth {
            Auth::ApiKey(secret) => (AuthKind::ApiKey, secret.as_str()),
            Auth::OAuth { refresh_token, .. } => (
                AuthKind::Oauth,
                refresh_token
                    .as_deref()
                    .ok_or(ConnectError::MissingRefreshToken)?,
            ),
        };
        self.storage
            .insert_provider(&provider_id, &kind, secret, "", "")
            .await?;

        // then we try to init the new runtime.
        match self
            .provider_controller
            .new_provider(provider_id, &auth)
            .await
        {
            Some(client) => {
                if client.health_statue() == HealthStatus::Unhealthy {
                    // cleanup from provider and the db
                    self.provider_controller.remove_provider(&provider_id);
                    self.storage.delete_provider(&provider_id).await?;
                    return Err(ConnectError::FailToInit(
                        client.error().unwrap_or_else(|| "unknown error".into()),
                    ));
                }

                // Record the runtime's default model/effort as the stored
                // preferences; with no usable catalogue, fail the connection.
                let models = client.models().await.unwrap_or_default();
                match models.first() {
                    Some(default) => {
                        self.storage
                            .update_preferences(
                                &provider_id,
                                &default.id,
                                &default.default_reasoning_effort,
                            )
                            .await?;
                        Ok(())
                    },
                    None => {
                        self.provider_controller.remove_provider(&provider_id);
                        self.storage.delete_provider(&provider_id).await?;
                        Err(ConnectError::NoModelsAvailable(provider_id))
                    },
                }
            },
            None => {
                error!(
                    "duplicate initialization for provider {:?}. This indicate a bug.",
                    &provider_id
                );
                Err(ConnectError::FailToInit("Already Init.".into()))
            },
        }
    }

    pub async fn disconnect(&self, provider_id: ProviderId) -> Result<()> {
        self.storage.delete_provider(&provider_id).await?;
        self.provider_controller.remove_provider(&provider_id);
        Ok(())
    }

    pub async fn set_preferences(
        &self,
        provider_id: ProviderId,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        self.storage
            .update_preferences(&provider_id, model, effort)
            .await?;
        Ok(())
    }

    /// update preferred model with prefer model and effort
    pub async fn set_preferred(
        &self,
        provider_id: ProviderId,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        self.set_preferences(provider_id, model, effort).await?;
        self.storage.set_preferred(&provider_id).await?;
        Ok(())
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>> {
        let connected: HashMap<ProviderId, ConnectedProvider> = self
            .storage
            .connected_providers()
            .await?
            .into_iter()
            .map(|p| (p.provider_id, p))
            .collect();

        let mut statuses = self.provider_controller.available_providers().await;

        let mut ids: Vec<ProviderId> = self.handlers.iter().map(|entry| *entry.key()).collect();
        ids.sort_by_key(|id| id.to_string());

        let mut connectors = Vec::with_capacity(ids.len());
        for id in ids {
            // A connection needs both the stored prefs and a live runtime status.
            let connection = match (connected.get(&id), statuses.remove(&id)) {
                (Some(cred), Some(status)) => Some(ConnectorConnection {
                    preferred: cred.preferred,
                    prefer_model: cred.model.clone(),
                    prefer_effort: cred.effort.clone(),
                    status,
                }),
                _ => None,
            };
            connectors.push(Connector { id, connection });
        }
        Ok(connectors)
    }

    /// overall health level for all model connections.
    pub async fn health_level(&self) -> HealthLevel {
        let providers = self.provider_controller.available_providers().await;
        let healthy = providers
            .values()
            .filter(|status| status.status == HealthStatus::Running)
            .count();
        HealthLevel::from_counts(providers.len(), healthy)
    }

    fn handler(&self, provider_id: ProviderId) -> Result<Arc<dyn ProviderAuthenticator>> {
        self.handlers
            .get(&provider_id)
            .map(|h| Arc::clone(h.value()))
            .ok_or(ConnectError::UnknownProvider(provider_id))
    }
}

pub struct Connector {
    pub id: ProviderId,
    pub connection: Option<ConnectorConnection>,
}

pub struct ConnectorConnection {
    pub preferred: bool,
    pub prefer_model: String,
    pub prefer_effort: String,
    pub status: ProviderStatis,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("unknown provider: {0:?}")]
    UnknownProvider(ProviderId),

    #[error("OAuth response did not include a refresh_token")]
    MissingRefreshToken,

    #[error("provider {0:?} returned no models")]
    NoModelsAvailable(ProviderId),

    #[error("fail to init provider runtime: {0}")]
    FailToInit(String),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

type Result<T> = std::result::Result<T, ConnectError>;
