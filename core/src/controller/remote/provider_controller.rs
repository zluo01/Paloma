use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dashmap::DashMap;
use futures::future::join_all;
use log::error;
use scry_provider_protocol::v1::{
    Backend, BackendAuth, ChatRequest, ConnectionPayload, HandshakeResponse, Model, ProviderAuth,
    ProviderAuthMethod, provider_auth,
};
use uuid::Uuid;

use crate::{
    HealthLevel, HealthStatus, Plugin, PluginType, Transport,
    db::{AuthKind, ConnectedBackend, Storage, StorageError},
    entity::{Icon, ProviderBackendId},
    provider::{
        ANTHROPIC_PLUGIN, ChatStream, OPENAI_PLUGIN, ProviderConnectionError, ProviderInfo,
        ProviderPlugin,
    },
};

pub struct ProviderController {
    handlers: DashMap<String, ProviderHandler>,
    // contains detail about backend, and lookup key for handlers map
    backends: DashMap<ProviderBackendId, Backend>,
    storage: Storage,
}

impl ProviderController {
    /// service startup initialization
    pub async fn new(storage: Storage) -> Result<Self> {
        let connected_providers = connected_auths(&storage).await?;

        let provider_plugins = storage.plugins_by_type(PluginType::Provider).await?;

        // init in parallel
        let results = join_all(
            [&*ANTHROPIC_PLUGIN, &*OPENAI_PLUGIN]
                .into_iter()
                .chain(provider_plugins.iter())
                .map(|plugin| {
                    let storage = storage.clone();
                    let connected_providers = &connected_providers;
                    async move {
                        (
                            plugin.name.as_str(),
                            init_provider_plugin(plugin, storage, connected_providers).await,
                        )
                    }
                }),
        )
        .await;

        let handlers: DashMap<String, ProviderHandler> = DashMap::new();
        let backends: DashMap<ProviderBackendId, Backend> = DashMap::new();
        for (name, result) in results {
            match result {
                Ok((detail, provider)) => {
                    for backend in detail.backends {
                        backends.insert(
                            ProviderBackendId {
                                provider_id: detail.provider_id.clone(),
                                backend_id: backend.backend_id.clone(),
                            },
                            backend,
                        );
                    }
                    handlers.insert(
                        detail.provider_id,
                        ProviderHandler {
                            description: detail.description,
                            connection: provider,
                        },
                    );
                },
                Err(e) => error!("failed to initialize provider plugin {name}: {e}"),
            }
        }

        Ok(Self {
            handlers,
            backends,
            storage,
        })
    }

    fn handler(&self, provider_id: &str) -> Result<Arc<ProviderPlugin>> {
        self.handlers
            .get(provider_id)
            .map(|handler| Arc::clone(&handler.connection))
            .ok_or_else(|| ProviderControllerError::UnknownProvider(provider_id.to_string()))
    }

    pub async fn add_provider(&self, plugin: &Plugin) -> Result<()> {
        let (detail, provider) =
            init_provider_plugin(plugin, self.storage.clone(), &HashMap::new()).await?;

        self.storage
            .insert_plugin(
                &detail.provider_id,
                PluginType::Provider,
                Transport::Local,
                plugin.timeout,
                &plugin.env,
                &plugin.args,
                None,
            )
            .await?;

        for backend in detail.backends {
            self.backends.insert(
                ProviderBackendId {
                    provider_id: detail.provider_id.clone(),
                    backend_id: backend.backend_id.clone(),
                },
                backend,
            );
        }
        self.handlers.insert(
            detail.provider_id,
            ProviderHandler {
                description: detail.description,
                connection: provider,
            },
        );
        Ok(())
    }

    pub async fn update_provider(&self, plugin: &Plugin) -> Result<()> {
        // re-init the updated plugin with any auth the user already connected
        let connected_providers = connected_auths(&self.storage).await?;
        let (detail, provider) =
            init_provider_plugin(plugin, self.storage.clone(), &connected_providers).await?;

        if provider.health() != HealthStatus::Running {
            return Err(ProviderControllerError::FailToInitialize(
                detail.provider_id,
            ));
        }

        self.storage
            .update_plugin(
                &detail.provider_id,
                plugin.transport,
                plugin.timeout,
                &plugin.env,
                &plugin.args,
            )
            .await?;

        self.backends
            .retain(|id, _| id.provider_id != detail.provider_id);

        for backend in detail.backends {
            self.backends.insert(
                ProviderBackendId {
                    provider_id: detail.provider_id.clone(),
                    backend_id: backend.backend_id.clone(),
                },
                backend,
            );
        }
        self.handlers.insert(
            detail.provider_id,
            ProviderHandler {
                description: detail.description,
                connection: provider,
            },
        );
        Ok(())
    }

    pub async fn remove_provider(&self, name: &str) -> Result<()> {
        self.storage.delete_plugin(name).await?;
        if let Some((_, handler)) = self.handlers.remove(name) {
            handler.connection.shutdown();
        }
        self.backends.retain(|id, _| id.provider_id != name);
        Ok(())
    }
}

/// connection
impl ProviderController {
    pub async fn init(&self, provider_backend_id: ProviderBackendId) -> Result<ConnectionPayload> {
        let connection = self.handler(&provider_backend_id.provider_id)?;

        Ok(connection
            .init_connection(provider_backend_id.backend_id.clone())
            .await?)
    }

    pub async fn finalize(
        &self,
        provider_auth_method: ProviderAuthMethod,
        provider_backend_id: ProviderBackendId,
        payload: String,
    ) -> Result<()> {
        let connection = self.handler(&provider_backend_id.provider_id)?;
        let backend_id = provider_backend_id.backend_id.clone();
        let auth = connection
            .finalize_connection(provider_auth_method, backend_id.clone(), payload)
            .await?;

        // init to db first so during initialization, we have target to update for tokens.
        let (kind, secret) = match &auth.payload {
            Some(provider_auth::Payload::ApiKey(api_key)) => (AuthKind::ApiKey, api_key.as_str()),
            Some(provider_auth::Payload::RefreshToken(refresh_token)) => {
                (AuthKind::Oauth, refresh_token.as_str())
            },
            // A finalized auth with no payload is a plugin contract violation.
            None => return Err(ProviderConnectionError::UnexpectedResponse.into()),
        };

        // oauth refresh token is rotate-on-every-use,
        // so we need to add a record first such that on init
        // we have record available to write the latest refresh token
        self.storage
            .insert_backend(&provider_backend_id, &kind, secret, "", "")
            .await?;

        // then we try to init the new runtime.
        match connection
            .init_backend(
                backend_id.clone(),
                BackendAuth {
                    backend_id: backend_id.clone(),
                    auth: Some(auth),
                },
            )
            .await
        {
            Ok(_) => {
                // Record the runtime's default model/effort as the stored
                // preferences; with no usable catalogue, fail the connection.
                // fall to default here on error so we can aggregate the cleanup process
                let models = connection
                    .list_models(backend_id.clone())
                    .await
                    .unwrap_or_default();
                match models.first() {
                    Some(default) => {
                        self.storage
                            .update_preferences(
                                &provider_backend_id,
                                &default.id,
                                &default.default_reasoning_effort,
                            )
                            .await?;
                        Ok(())
                    },
                    None => {
                        self.storage.delete_backend(&provider_backend_id).await?;
                        connection.remove_backend(backend_id).await?;
                        Err(ProviderControllerError::NoModelsAvailable(
                            provider_backend_id,
                        ))
                    },
                }
            },
            Err(error) => {
                // cleanup from provider and the db
                self.storage.delete_backend(&provider_backend_id).await?;
                connection.remove_backend(backend_id).await?;
                Err(ProviderControllerError::from(error))
            },
        }
    }

    pub async fn cancel_connection(&self, provider_backend_id: ProviderBackendId) -> Result<()> {
        let connection = self.handler(&provider_backend_id.provider_id)?;
        Ok(connection
            .cancel_connection(provider_backend_id.backend_id)
            .await?)
    }

    pub async fn disconnect(&self, provider_backend_id: ProviderBackendId) -> Result<()> {
        self.storage.delete_backend(&provider_backend_id).await?;
        let connection = self.handler(&provider_backend_id.provider_id)?;
        Ok(connection
            .remove_backend(provider_backend_id.backend_id)
            .await?)
    }

    pub async fn set_preferred(
        &self,
        provider_backend_id: ProviderBackendId,
        model: &str,
        effort: &str,
        as_default: bool,
    ) -> Result<()> {
        self.storage
            .set_preferred_provider_backend_config(&provider_backend_id, model, effort, as_default)
            .await?;
        Ok(())
    }

    pub async fn prefer_provider(&self) -> Result<Option<ProviderBackendId>> {
        Ok(self.storage.preferred_provider_backend_id().await?)
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>> {
        let connected: HashMap<ProviderBackendId, ConnectedBackend> = self
            .storage
            .connected_backends()
            .await?
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect();

        let mut statuses = self
            .available_backends(Some(connected.keys().cloned().collect()))
            .await?;

        let mut connectors: Vec<Connector> = self
            .backends
            .iter()
            .map(|entry| {
                let id = entry.key();
                let backend = entry.value();
                // A connection needs both the stored prefs and a live runtime status.
                let connection = match (connected.get(id), statuses.remove(id)) {
                    (Some(cred), Some(status)) => Some(ConnectorConnection {
                        preferred: cred.preferred,
                        prefer_model: cred.model.clone(),
                        prefer_effort: cred.effort.clone(),
                        status,
                    }),
                    _ => None,
                };
                Connector {
                    id: id.clone(),
                    description: backend.description.clone(),
                    icon: backend.icon.clone().map(Icon::Embedded),
                    connection,
                }
            })
            .collect();
        connectors.sort_by(|a, b| {
            (&a.id.provider_id, &a.id.backend_id).cmp(&(&b.id.provider_id, &b.id.backend_id))
        });
        Ok(connectors)
    }

    /// overall health level for all model connections.
    pub async fn backends_health_level(&self) -> HealthLevel {
        let plugins: Vec<Arc<ProviderPlugin>> = self
            .handlers
            .iter()
            .map(|entry| Arc::clone(&entry.value().connection))
            .collect();

        let statuses: Vec<HealthStatus> =
            join_all(plugins.into_iter().map(|connection| async move {
                connection
                    .backend_health_status()
                    .await
                    .unwrap_or_else(|_| vec![HealthStatus::Unhealthy])
            }))
            .await
            .into_iter()
            .flatten()
            .collect();

        let healthy = statuses
            .iter()
            .filter(|status| **status == HealthStatus::Running)
            .count();
        HealthLevel::from_counts(statuses.len(), healthy)
    }
}

/// runtime
impl ProviderController {
    pub async fn available_backends(
        &self,
        connected: Option<Vec<ProviderBackendId>>,
    ) -> Result<HashMap<ProviderBackendId, ProviderStatus>> {
        let connected: HashSet<ProviderBackendId> = match connected {
            None => self
                .storage
                .connected_backends()
                .await?
                .into_iter()
                .map(|o| o.id)
                .collect(),
            Some(connected) => connected.into_iter().collect(),
        };

        let backends: Vec<(ProviderBackendId, Arc<ProviderPlugin>)> = self
            .backends
            .iter()
            .filter(|entry| connected.contains(entry.key()))
            .filter_map(|entry| {
                self.handlers
                    .get(&entry.key().provider_id)
                    .map(|handler| (entry.key().clone(), Arc::clone(&handler.connection)))
            })
            .collect();

        let statuses = join_all(backends.into_iter().map(|(id, connection)| async move {
            let backend_id = id.backend_id.clone();
            let status = connection
                .health_status(backend_id.clone())
                .await
                .unwrap_or(HealthStatus::Unhealthy);
            let models = connection
                .list_models(backend_id.clone())
                .await
                .unwrap_or_default();
            let error = connection.error(backend_id).await.unwrap_or_default();
            (
                id,
                ProviderStatus {
                    models,
                    status,
                    error,
                },
            )
        }))
        .await
        .into_iter()
        .collect();

        Ok(statuses)
    }

    pub fn health_level(&self) -> HealthLevel {
        HealthLevel::combine(
            self.handlers
                .iter()
                .map(|entry| entry.connection.health().into()),
        )
    }

    pub async fn available_providers(&self) -> Result<Vec<ProviderInfo>> {
        let plugins = self.storage.plugins_by_type(PluginType::Provider).await?;
        Ok([&*ANTHROPIC_PLUGIN, &*OPENAI_PLUGIN]
            .into_iter()
            .map(|builtin| (builtin.name.clone(), None))
            .chain(
                plugins
                    .into_iter()
                    .map(|config| (config.name.clone(), Some(config))),
            )
            .filter_map(|(name, config)| {
                let Some(handler) = self.handlers.get(&name) else {
                    error!(
                        "no live provider status for plugin {name}; This indicates a bug. Skipping"
                    );
                    return None;
                };
                Some(ProviderInfo {
                    name,
                    description: handler.description.clone(),
                    status: handler.connection.health(),
                    error: handler.connection.plugin_error(),
                    config,
                })
            })
            .collect())
    }
}

/// chat
impl ProviderController {
    pub async fn chat(
        &self,
        provider_backend_id: ProviderBackendId,
        chat_request: ChatRequest,
    ) -> Result<ChatStream> {
        let connection = self.handler(&provider_backend_id.provider_id)?;
        Ok(connection
            .chat(provider_backend_id.backend_id, chat_request)
            .await?)
    }

    pub async fn cancel_chat(
        &self,
        provider_backend_id: ProviderBackendId,
        session_id: Uuid,
    ) -> Result<()> {
        let connection = self.handler(&provider_backend_id.provider_id)?;

        Ok(connection
            .cancel_chat(provider_backend_id.backend_id, session_id.to_string())
            .await?)
    }
}

async fn connected_auths(storage: &Storage) -> Result<HashMap<ProviderBackendId, ProviderAuth>> {
    Ok(storage
        .connected_backends()
        .await?
        .into_iter()
        .map(|cred| {
            let payload = match cred.auth_kind {
                AuthKind::ApiKey => provider_auth::Payload::ApiKey(cred.secret),
                AuthKind::Oauth => provider_auth::Payload::RefreshToken(cred.secret),
            };
            (
                cred.id,
                ProviderAuth {
                    payload: Some(payload),
                },
            )
        })
        .collect())
}

async fn init_provider_plugin(
    plugin: &Plugin,
    storage: Storage,
    connected_providers: &HashMap<ProviderBackendId, ProviderAuth>,
) -> Result<(HandshakeResponse, Arc<ProviderPlugin>)> {
    let provider = ProviderPlugin::connect(plugin, storage)?;

    let provider_detail = provider.handshake().await?;

    let auths: Vec<BackendAuth> = provider_detail
        .backends
        .iter()
        .filter_map(|backend| {
            connected_providers
                .get(&ProviderBackendId {
                    provider_id: provider_detail.provider_id.clone(),
                    backend_id: backend.backend_id.clone(),
                })
                .map(|auth| BackendAuth {
                    backend_id: backend.backend_id.clone(),
                    auth: Some(auth.clone()),
                })
        })
        .collect();

    provider.init_backends(auths).await?;

    Ok((provider_detail, provider))
}

struct ProviderHandler {
    description: String,
    connection: Arc<ProviderPlugin>,
}

#[derive(Clone)]
pub struct Connector {
    pub id: ProviderBackendId,
    pub description: String,
    pub icon: Option<Icon>,
    pub connection: Option<ConnectorConnection>,
}

#[derive(Clone)]
pub struct ConnectorConnection {
    pub preferred: bool,
    pub prefer_model: String,
    pub prefer_effort: String,
    pub status: ProviderStatus,
}

#[derive(Clone)]
pub struct ProviderStatus {
    pub models: Vec<Model>,
    pub status: HealthStatus,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderControllerError {
    #[error("provider plugin {0} is not registered")]
    UnknownProvider(String),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Connection(#[from] ProviderConnectionError),

    #[error("provider {0:?} returned no models")]
    NoModelsAvailable(ProviderBackendId),

    #[error("OAuth response did not include a refresh_token")]
    MissingRefreshToken,

    #[error("fail to initialize provider plugin: {0}")]
    FailToInitialize(String),
}

type Result<T> = std::result::Result<T, ProviderControllerError>;
