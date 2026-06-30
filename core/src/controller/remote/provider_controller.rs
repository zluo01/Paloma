use std::{collections::HashMap, sync::Arc};

use dashmap::DashMap;
use futures::future::join_all;
use log::error;

use crate::{
    db::{AuthKind, Storage, StorageError},
    entity::{HealthStatus, ProviderId},
    provider::{AnthropicRuntime, Auth, CodexRuntime, Model, OpenAIRuntime, ProviderClient},
};

pub struct ProviderController {
    handlers: DashMap<ProviderId, Arc<dyn ProviderClient>>,
    storage: Storage,
    request: reqwest::Client,
}

impl ProviderController {
    /// service startup initialization
    pub async fn new(
        storage: Storage,
        request: reqwest::Client,
    ) -> Result<Self, ProviderControllerError> {
        let handlers: DashMap<ProviderId, Arc<dyn ProviderClient>> = DashMap::new();

        let clients = join_all(
            storage
                .connected_providers()
                .await?
                .into_iter()
                .map(|cred| {
                    let request = request.clone();
                    let storage = storage.clone();
                    async move {
                        let auth = match cred.auth_kind {
                            AuthKind::ApiKey => Auth::ApiKey(cred.secret),
                            AuthKind::Oauth => Auth::OAuth {
                                refresh_token: Some(cred.secret),
                                expires_at: None,
                            },
                        };
                        let client: Arc<dyn ProviderClient> = match cred.provider_id {
                            ProviderId::Codex => {
                                Arc::new(CodexRuntime::new(&auth, request, storage).await)
                            },
                            ProviderId::OpenAI => {
                                Arc::new(OpenAIRuntime::new(&auth, request).await)
                            },
                            ProviderId::Anthropic => {
                                Arc::new(AnthropicRuntime::new(&auth, request).await)
                            },
                            _ => todo!(),
                        };
                        (client.id(), client)
                    }
                }),
        )
        .await;

        for (provider_id, client) in clients {
            handlers.insert(provider_id, client);
        }

        Ok(Self {
            handlers,
            storage,
            request,
        })
    }

    /// runtime add new connection
    pub async fn new_provider(
        &self,
        provider_id: ProviderId,
        auth: &Auth,
    ) -> Option<Arc<dyn ProviderClient>> {
        // fail early such that we do not hold the shard lock for the provider in dashmap.
        if self.handlers.contains_key(&provider_id) {
            error!("provider {provider_id:?} already registered; ignoring");
            return None;
        }

        let client: Arc<dyn ProviderClient> = match provider_id {
            ProviderId::Codex => {
                Arc::new(CodexRuntime::new(auth, self.request.clone(), self.storage.clone()).await)
            },
            ProviderId::OpenAI => Arc::new(OpenAIRuntime::new(auth, self.request.clone()).await),
            ProviderId::Anthropic => {
                Arc::new(AnthropicRuntime::new(auth, self.request.clone()).await)
            },
            _ => todo!(),
        };

        self.handlers.insert(provider_id, client.clone());
        Some(client)
    }

    pub fn remove_provider(&self, provider_id: &ProviderId) {
        self.handlers.remove(provider_id);
    }

    /// Per-provider status (models, health, error) for every registered
    /// runtime client.
    pub async fn available_providers(&self) -> HashMap<ProviderId, ProviderStatis> {
        let clients: Vec<(ProviderId, Arc<dyn ProviderClient>)> = self
            .handlers
            .iter()
            .map(|entry| (*entry.key(), Arc::clone(entry.value())))
            .collect();

        let mut providers = HashMap::with_capacity(clients.len());
        for (id, client) in clients {
            providers.insert(
                id,
                ProviderStatis {
                    model: client.models().await.unwrap_or_default(),
                    status: client.health_statue(),
                    error: client.error(),
                },
            );
        }
        providers
    }

    pub fn client(
        &self,
        provider_id: ProviderId,
    ) -> Result<Arc<dyn ProviderClient>, ProviderControllerError> {
        self.handlers
            .get(&provider_id)
            .map(|h| Arc::clone(h.value()))
            .ok_or(ProviderControllerError::UnknownProvider(provider_id))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderControllerError {
    #[error("provider not registered: {0:?}")]
    UnknownProvider(ProviderId),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub struct ProviderStatis {
    pub model: Vec<Model>,
    pub status: HealthStatus,
    pub error: Option<String>,
}
