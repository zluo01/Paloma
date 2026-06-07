use std::sync::Arc;

use dashmap::{DashMap, Entry};
use log::error;
use scry_provider::{Auth, CodexRuntime, Model, ProviderClient, ProviderError, ProviderId};
use scry_storage::{Storage, StorageError};
use tokio::task::JoinHandle;

use crate::remote::SessionManagerError;

pub struct ProviderController {
    handlers: DashMap<ProviderId, Arc<dyn ProviderClient>>,
    refresh_handles: DashMap<ProviderId, JoinHandle<()>>,
    storage: Storage,
    request: reqwest::Client,
}

impl ProviderController {
    pub async fn new(
        storage: Storage,
        request: reqwest::Client,
    ) -> Result<Self, ProviderControllerError> {
        let handlers: DashMap<ProviderId, Arc<dyn ProviderClient>> = DashMap::new();
        let refresh_handles: DashMap<ProviderId, JoinHandle<()>> = DashMap::new();

        for cred in storage.connected_providers().await? {
            let auth = match cred.auth_kind.as_str() {
                "api_key" => Auth::ApiKey(cred.secret),
                "oauth" => Auth::OAuth {
                    refresh_token: Some(cred.secret),
                    expires_in: None,
                },
                other => {
                    error!("startup: find unknown auth_kind {other:?}");
                    continue;
                },
            };
            let client: Arc<dyn ProviderClient> = match cred.provider_id.as_str() {
                "codex" => Arc::new(CodexRuntime::new(&auth, request.clone()).await?),
                other => {
                    error!("startup: get unknown provider_id {other:?}");
                    continue;
                },
            };
            refresh_and_schedule(
                client.id(),
                client.clone(),
                storage.clone(),
                &refresh_handles,
            )
            .await;
            handlers.insert(client.id(), client.clone());
        }

        Ok(Self {
            handlers,
            refresh_handles,
            storage,
            request,
        })
    }

    pub async fn new_provider(
        &self,
        provider_id: ProviderId,
        auth: &Auth,
    ) -> Result<(), ProviderControllerError> {
        let client: Arc<dyn ProviderClient> = match provider_id {
            ProviderId::Codex => Arc::new(CodexRuntime::new(auth, self.request.clone()).await?),
        };
        match self.handlers.entry(provider_id) {
            Entry::Occupied(_) => {
                error!("provider {provider_id:?} already registered; ignoring");
                Err(ProviderControllerError::AlreadyRegistered(provider_id))
            },
            Entry::Vacant(slot) => {
                refresh_and_schedule(
                    provider_id,
                    client.clone(),
                    self.storage.clone(),
                    &self.refresh_handles,
                )
                .await;
                slot.insert(client);
                Ok(())
            },
        }
    }

    pub fn remove_provider(&self, provider_id: ProviderId) {
        self.handlers.remove(&provider_id);
        if let Some((_, handle)) = self.refresh_handles.remove(&provider_id) {
            handle.abort();
        }
    }

    pub async fn provider_models(&self, provider_id: ProviderId) -> Option<Vec<Model>> {
        let client = match self.client(provider_id) {
            Ok(c) => c,
            Err(e) => {
                log::error!("missing proper client: {e}");
                return None;
            },
        };
        match client.models().await {
            Ok(m) => Some(m),
            Err(e) => {
                log::error!("models: failed to fetch for {provider_id:?}: {e}");
                None
            },
        }
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

    #[error("provider already registered: {0:?}")]
    AlreadyRegistered(ProviderId),

    #[error("session writer channel closed")]
    WriterChannelClosed,

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),
}

// TODO we should refresh on start up and check if error happens,
//  if error happens, we should return a special state
//  such that UI end can show some reconnect actions for user to fix.
async fn refresh_and_schedule(
    provider_id: ProviderId,
    client: Arc<dyn ProviderClient>,
    storage: Storage,
    refresh_handles: &DashMap<ProviderId, JoinHandle<()>>,
) {
    if refresh_handles.contains_key(&provider_id) {
        return;
    }

    let initial_expires_in = match client.refresh(&storage).await {
        Ok(Some(Auth::OAuth {
            expires_in: Some(s),
            ..
        })) => s,
        Ok(_) => return,
        Err(e) => {
            error!("initial refresh failed: {e}");
            return;
        },
    };

    let handle = tokio::spawn(async move {
        let mut expires_in = initial_expires_in;
        loop {
            let wait = expires_in.saturating_sub(60).max(0) as u64;
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            match client.refresh(&storage).await {
                Ok(Some(Auth::OAuth {
                    expires_in: Some(s),
                    ..
                })) => expires_in = s,
                Ok(_) => return,
                Err(e) => {
                    error!("scheduled refresh failed: {e}");
                    return;
                },
            }
        }
    });

    refresh_handles.insert(provider_id, handle);
}
