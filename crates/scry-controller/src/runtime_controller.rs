use std::sync::Arc;

use dashmap::{DashMap, Entry};
use log::error;
use scry_provider::entity::{Auth, Model, ProviderClient, ProviderError, ProviderId};
use scry_provider::runtime::CodexRuntime;
use scry_storage::storage::Storage;
use scry_storage::StorageError;

pub struct RuntimeController {
    handlers: DashMap<ProviderId, Arc<dyn ProviderClient>>,
    request: reqwest::Client,
}

impl RuntimeController {
    pub async fn new(
        storage: &Storage,
        request: reqwest::Client,
    ) -> Result<Self, RuntimeControllerError> {
        let handlers: DashMap<ProviderId, Arc<dyn ProviderClient>> = DashMap::new();

        for cred in storage.connected_providers().await? {
            let auth = match cred.auth_kind.as_str() {
                "api_key" => Auth::ApiKey(cred.secret),
                "oauth" => Auth::OAuth {
                    refresh_token: Some(cred.secret),
                    expires_at_unix: cred.expires_at,
                },
                other => {
                    error!("startup: find unknown auth_kind {other:?}");
                    continue;
                }
            };
            let client: Arc<dyn ProviderClient> = match cred.provider_id.as_str() {
                "codex" => Arc::new(CodexRuntime::new(&auth, request.clone()).await?),
                other => {
                    error!("startup: get unknown provider_id {other:?}");
                    continue;
                }
            };
            handlers.insert(client.id(), client);
        }

        Ok(Self { handlers, request })
    }

    pub async fn new_model(
        &self,
        provider_id: ProviderId,
        auth: &Auth,
    ) -> Result<(), RuntimeControllerError> {
        let client: Arc<dyn ProviderClient> = match provider_id {
            ProviderId::Codex => Arc::new(CodexRuntime::new(auth, self.request.clone()).await?),
        };
        match self.handlers.entry(provider_id) {
            Entry::Occupied(_) => {
                error!("new_model: provider {provider_id:?} already registered; ignoring");
                Err(RuntimeControllerError::AlreadyRegistered(provider_id))
            }
            Entry::Vacant(slot) => {
                slot.insert(client);
                Ok(())
            }
        }
    }

    pub fn remove_model(&self, provider_id: ProviderId) {
        self.handlers.remove(&provider_id);
    }

    pub async fn models(&self, provider_id: ProviderId) -> Option<Vec<Model>> {
        let client = match self.client(provider_id) {
            Ok(c) => c,
            Err(e) => {
                log::error!("missing proper client: {e}");
                return None;
            }
        };
        match client.models().await {
            Ok(m) => Some(m),
            Err(e) => {
                log::error!("models: failed to fetch for {provider_id:?}: {e}");
                None
            }
        }
    }

    fn client(
        &self,
        provider_id: ProviderId,
    ) -> Result<Arc<dyn ProviderClient>, RuntimeControllerError> {
        self.handlers
            .get(&provider_id)
            .map(|h| Arc::clone(h.value()))
            .ok_or(RuntimeControllerError::UnknownProvider(provider_id))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeControllerError {
    #[error("provider not registered: {0:?}")]
    UnknownProvider(ProviderId),

    #[error("provider already registered: {0:?}")]
    AlreadyRegistered(ProviderId),

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}
