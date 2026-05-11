use std::sync::Arc;

use dashmap::DashMap;

use scry_provider::connector::CodexConnector;
use scry_provider::entity::{Auth, Connection, ProviderAuthenticator, ProviderError};
use scry_storage::storage::Storage;
use scry_storage::StorageError;

pub struct ConnectController {
    handlers: DashMap<&'static str, Arc<dyn ProviderAuthenticator>>,
    storage: Storage,
}

impl ConnectController {
    pub fn new(storage: Storage, http: reqwest::Client) -> Self {
        let handlers: DashMap<&'static str, Arc<dyn ProviderAuthenticator>> = DashMap::new();

        let codex: Arc<dyn ProviderAuthenticator> = Arc::new(CodexConnector::new(http));
        handlers.insert(codex.id(), codex);

        Self { handlers, storage }
    }

    pub async fn init(&self, provider_id: &str) -> Result<Connection, ConnectError> {
        let handler = self.handler(provider_id)?;
        Ok(handler.init_connection().await?)
    }

    pub async fn finalize(
        &self,
        provider_id: &str,
        payload: Connection,
    ) -> Result<Auth, ConnectError> {
        let handler = self.handler(provider_id)?;
        let auth = handler.finalize_connection(payload).await?;
        self.persist(provider_id, &auth).await?;
        Ok(auth)
    }

    fn handler(&self, provider_id: &str) -> Result<Arc<dyn ProviderAuthenticator>, ConnectError> {
        self.handlers
            .get(provider_id)
            .map(|h| Arc::clone(h.value()))
            .ok_or_else(|| ConnectError::UnknownProvider(provider_id.to_string()))
    }

    async fn persist(&self, provider_id: &str, auth: &Auth) -> Result<(), ConnectError> {
        let kind = auth.kind_str();
        match auth {
            Auth::ApiKey(secret) => {
                self.storage
                    .insert_provider(provider_id, kind, secret, None)
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
                    .insert_provider(provider_id, kind, secret, *expires_at_unix)
                    .await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("unknown provider: {0}")]
    UnknownProvider(String),

    #[error("OAuth response did not include a refresh_token")]
    MissingRefreshToken,

    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}
