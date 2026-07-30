use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use paloma_provider_protocol::v1::ConversationItem;
use serde_json::Value;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use super::{AuthKind, queries};
use crate::{
    db::entity::{
        ConnectedBackend, HistoryEntry, Permission, PreferModelConfig, RestoreEntry, Session,
    },
    entity::{CapabilityFacet, Plugin, PluginArgs, PluginType, ProviderBackendId, Transport},
};

#[derive(Clone)]
pub struct Storage {
    pool: Pool<Sqlite>,
}

impl Storage {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let pool = create_pool(db_path).await?;
        Self::from_pool(pool).await
    }

    pub async fn from_pool(pool: Pool<Sqlite>) -> Result<Self> {
        initialize(&pool).await?;
        Ok(Self { pool })
    }

    #[cfg(test)]
    fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn insert_backend(
        &self,
        provider_backend_id: &ProviderBackendId,
        auth_kind: &AuthKind,
        secret: &str,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_BACKEND_QUERY)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .bind(auth_kind)
            .bind(secret)
            .bind(model)
            .bind(effort)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::DuplicateBackend(provider_backend_id.clone())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn update_backend(
        &self,
        provider_backend_id: &ProviderBackendId,
        auth_kind: &AuthKind,
        secret: &str,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_BACKEND_QUERY)
            .bind(auth_kind)
            .bind(secret)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundBackend(provider_backend_id.clone()));
        }
        Ok(())
    }

    pub async fn update_preferences(
        &self,
        provider_backend_id: &ProviderBackendId,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_BACKEND_PREFERENCES_QUERY)
            .bind(model)
            .bind(effort)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundBackend(provider_backend_id.clone()));
        }
        Ok(())
    }

    pub async fn set_preferred_provider_backend_config(
        &self,
        provider_backend_id: &ProviderBackendId,
        model: &str,
        effort: &str,
        as_default: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(queries::UPDATE_BACKEND_PREFERENCES_QUERY)
            .bind(model)
            .bind(effort)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundBackend(provider_backend_id.clone()));
        }

        if as_default {
            sqlx::query(queries::SET_PREFERRED_QUERY)
                .bind(provider_backend_id.provider_id.as_str())
                .bind(provider_backend_id.backend_id.as_str())
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_backend(&self, provider_backend_id: &ProviderBackendId) -> Result<()> {
        let result = sqlx::query(queries::DELETE_BACKEND_QUERY)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundBackend(provider_backend_id.clone()));
        }
        Ok(())
    }

    pub async fn create_new_session(&self, session_id: Uuid, title: &str) -> Result<()> {
        sqlx::query(queries::CREATE_NEW_SESSION_QUERY)
            .bind(session_id.to_string())
            .bind(title)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::DuplicateSession(session_id.to_string())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn all_sessions(&self) -> Result<Vec<Session>> {
        let sessions = sqlx::query_as::<_, Session>(queries::GET_ALL_SESSIONS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
    }

    pub async fn search_sessions(&self, needle: &str) -> Result<Vec<String>> {
        let pattern = format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let sessions = sqlx::query_scalar::<_, String>(queries::SEARCH_SESSIONS_QUERY)
            .bind(&pattern)
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let result = sqlx::query(queries::DELETE_SESSION_QUERY)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundSession(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn connected_backends(&self) -> Result<Vec<ConnectedBackend>> {
        let providers = sqlx::query_as::<_, ConnectedBackend>(queries::CONNECTED_BACKENDS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(providers)
    }

    pub async fn preferred_provider_backend_id(&self) -> Result<Option<ProviderBackendId>> {
        Ok(
            sqlx::query_as::<_, ProviderBackendId>(queries::PREFERRED_BACKEND_QUERY)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn prefer_model_config(
        &self,
        provider_backend_id: &ProviderBackendId,
    ) -> Result<PreferModelConfig> {
        sqlx::query_as::<_, PreferModelConfig>(queries::PREFER_MODEL_CONFIG_QUERY)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StorageError::NotFoundBackend(provider_backend_id.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_plugin(
        &self,
        name: &str,
        plugin_type: PluginType,
        transport: Transport,
        timeout: u32,
        env: &HashMap<String, String>,
        args: &PluginArgs,
        credential: Option<&Value>,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_PLUGIN_QUERY)
            .bind(name)
            .bind(plugin_type)
            .bind(transport)
            .bind(timeout)
            .bind(serde_json::to_string(env)?)
            .bind(serde_json::to_string(args)?)
            .bind(credential.map(serde_json::to_string).transpose()?)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::DuplicatePlugin(name.to_string())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn update_plugin(
        &self,
        name: &str,
        transport: Transport,
        timeout: u32,
        env: &HashMap<String, String>,
        args: &PluginArgs,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_PLUGIN_QUERY)
            .bind(transport)
            .bind(timeout)
            .bind(serde_json::to_string(env)?)
            .bind(serde_json::to_string(args)?)
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundPlugin(name.to_string()));
        }
        Ok(())
    }

    pub async fn set_plugin_credential(
        &self,
        name: &str,
        credential: Option<&Value>,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_PLUGIN_CREDENTIAL_QUERY)
            .bind(credential.map(serde_json::to_string).transpose()?)
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundPlugin(name.to_string()));
        }
        Ok(())
    }

    pub async fn plugins_by_type(&self, plugin_type: PluginType) -> Result<Vec<Plugin>> {
        let plugins = sqlx::query_as::<_, Plugin>(queries::GET_PLUGINS_BY_TYPE_QUERY)
            .bind(plugin_type)
            .fetch_all(&self.pool)
            .await?;
        Ok(plugins)
    }

    pub async fn plugin_credential(&self, name: &str) -> Result<Option<Value>> {
        let credential =
            sqlx::query_scalar::<_, Option<Value>>(queries::GET_PLUGIN_CREDENTIAL_QUERY)
                .bind(name)
                .fetch_optional(&self.pool)
                .await?;
        Ok(credential.flatten())
    }

    pub async fn delete_plugin(&self, name: &str) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PLUGIN_QUERY)
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundPlugin(name.to_string()));
        }
        Ok(())
    }

    pub async fn toggle_plugin(&self, name: &str, disabled: bool) -> Result<()> {
        let result = sqlx::query(queries::DISABLE_PLUGIN_QUERY)
            .bind(disabled)
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundPlugin(name.to_string()));
        }
        Ok(())
    }

    pub async fn disabled_plugins(&self) -> Result<HashSet<String>> {
        let names = sqlx::query_scalar::<_, String>(queries::DISABLED_PLUGINS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(names.into_iter().collect())
    }

    pub async fn toggle_capability(
        &self,
        plugin_name: &str,
        capability_id: &str,
        facet: CapabilityFacet,
        disabled: bool,
    ) -> Result<()> {
        let query = if disabled {
            queries::DISABLE_CAPABILITY_QUERY
        } else {
            queries::ENABLE_CAPABILITY_QUERY
        };
        sqlx::query(query)
            .bind(plugin_name)
            .bind(capability_id)
            .bind(facet)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn disabled_capabilities(
        &self,
        facets: &[CapabilityFacet],
    ) -> Result<HashSet<(String, String, CapabilityFacet)>> {
        let rows = sqlx::query_as::<_, (String, String, CapabilityFacet)>(
            queries::DISABLED_CAPABILITIES_QUERY,
        )
        .bind(serde_json::to_string(facets)?)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    pub async fn is_command_allowed(&self, command: &str) -> Result<bool> {
        let allowed: bool = sqlx::query_scalar(queries::MATCH_PERMISSION_QUERY)
            .bind(command)
            .bind(command)
            .fetch_one(&self.pool)
            .await?;
        Ok(allowed)
    }

    pub async fn add_permission(&self, prefix: &str, with_glob: bool) -> Result<()> {
        sqlx::query(queries::INSERT_PERMISSION_QUERY)
            .bind(prefix)
            .bind(with_glob)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_permissions(&self) -> Result<Vec<Permission>> {
        let permissions = sqlx::query_as::<_, Permission>(queries::GET_PERMISSIONS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(permissions)
    }

    pub async fn delete_permission(&self, prefix: &str) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PERMISSION_QUERY)
            .bind(prefix)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFoundPermission(prefix.to_string()));
        }
        Ok(())
    }

    pub async fn insert_history(
        &self,
        session_id: &str,
        provider_backend_id: &ProviderBackendId,
        payload: &ConversationItem,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_HISTORY)
            .bind(session_id)
            .bind(provider_backend_id.provider_id.as_str())
            .bind(provider_backend_id.backend_id.as_str())
            .bind(payload.payload_type())
            .bind(serde_json::to_string(payload)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_history(&self, session_id: &str) -> Result<Vec<HistoryEntry>> {
        Ok(sqlx::query_as::<_, HistoryEntry>(queries::GET_HISTORY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn restore_history(&self, session_id: &str) -> Result<Vec<RestoreEntry>> {
        let entries = sqlx::query_as::<_, RestoreEntry>(queries::RESTORE_HISTORY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(entries)
    }

    /// Prune partially-written turns left by a crash or cold start: for every
    /// session whose newest history item isn't an assistant message, drop
    /// everything back to (and including) the last user prompt.
    pub async fn recover_history(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(queries::RECOVER).execute(&mut *tx).await?;
        sqlx::query(queries::DELETE_ALL_EMPTY_SESSIONS)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Roll one session back to its last completed assistant message, then delete
    /// the session if no history remains. Returns whether the session was removed.
    pub async fn rollback_session_history(&self, session_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(queries::ROLLBACK)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        let result = sqlx::query(queries::DELETE_EMPTY_SESSION)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }
}

async fn create_pool(db_path: &Path) -> Result<Pool<Sqlite>> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await?;
    Ok(pool)
}

async fn initialize(pool: &Pool<Sqlite>) -> Result<()> {
    sqlx::query(queries::INIT_TABLE_QUERY).execute(pool).await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("backend {0} not found")]
    NotFoundBackend(ProviderBackendId),

    #[error("session {0} not found")]
    NotFoundSession(String),

    #[error("plugin {0} not found")]
    NotFoundPlugin(String),

    #[error("permission {0} not found")]
    NotFoundPermission(String),

    #[error("backend {0} already exists")]
    DuplicateBackend(ProviderBackendId),

    #[error("session {0} already exists")]
    DuplicateSession(String),

    #[error("plugin {0} already exists")]
    DuplicatePlugin(String),
}

type Result<T> = std::result::Result<T, StorageError>;

#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
