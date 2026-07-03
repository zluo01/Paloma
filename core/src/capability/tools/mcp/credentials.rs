use async_trait::async_trait;
use rmcp::transport::{AuthError, CredentialStore, StoredCredentials};

use crate::db::Storage;

#[derive(Clone)]
pub(super) struct CredentialStorage {
    storage: Storage,
    name: String,
}

impl CredentialStorage {
    pub fn new(storage: Storage, name: String) -> Self {
        Self { storage, name }
    }
}

fn internal(error: impl std::fmt::Display) -> AuthError {
    AuthError::InternalError(error.to_string())
}

#[async_trait]
impl CredentialStore for CredentialStorage {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        self.storage
            .plugin_credential(&self.name)
            .await
            .map_err(internal)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(internal)
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let value = serde_json::to_value(&credentials).map_err(internal)?;
        self.storage
            .set_plugin_credential(&self.name, Some(&value))
            .await
            .map_err(internal)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.storage
            .set_plugin_credential(&self.name, None)
            .await
            .map_err(internal)
    }
}
