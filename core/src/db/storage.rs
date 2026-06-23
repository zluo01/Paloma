use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use serde_json::Value;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use super::{AuthKind, queries};
use crate::{
    db::entity::{
        ConnectedProvider, EntryType, FileEntry, PreferModelConfig, RestoreEntry, Session,
    },
    entity::{Plugin, PluginArgs, PluginType, ProviderId, Transport},
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

    pub async fn insert_provider(
        &self,
        provider_id: &ProviderId,
        auth_kind: &AuthKind,
        secret: &str,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_PROVIDER_QUERY)
            .bind(provider_id)
            .bind(auth_kind)
            .bind(secret)
            .bind(model)
            .bind(effort)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::Duplicate(provider_id.to_string())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn update_provider(
        &self,
        provider_id: &ProviderId,
        auth_kind: &AuthKind,
        secret: &str,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_PROVIDER_QUERY)
            .bind(auth_kind)
            .bind(secret)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(provider_id.to_string()));
        }
        Ok(())
    }

    pub async fn update_preferences(
        &self,
        provider_id: &ProviderId,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_PROVIDER_PREFERENCES_QUERY)
            .bind(model)
            .bind(effort)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(provider_id.to_string()));
        }
        Ok(())
    }

    pub async fn delete_provider(&self, provider_id: &ProviderId) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PROVIDER_QUERY)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(provider_id.to_string()));
        }
        Ok(())
    }

    pub async fn set_preferred(&self, provider_id: &ProviderId) -> Result<()> {
        sqlx::query(queries::SET_PREFERRED_QUERY)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_new_session(
        &self,
        session_id: Uuid,
        provider_id: &ProviderId,
        title: &str,
    ) -> Result<()> {
        sqlx::query(queries::CREATE_NEW_SESSION_QUERY)
            .bind(session_id.to_string())
            .bind(provider_id)
            .bind(title)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::Duplicate(session_id.to_string())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn touch_session(&self, session_id: &str) -> Result<()> {
        let result = sqlx::query(queries::TOUCH_SESSION_QUERY)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn all_sessions(&self) -> Result<Vec<Session>> {
        let sessions = sqlx::query_as::<_, Session>(queries::GET_ALL_SESSIONS_QUERY)
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
            return Err(StorageError::NotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn connected_providers(&self) -> Result<Vec<ConnectedProvider>> {
        let providers = sqlx::query_as::<_, ConnectedProvider>(queries::CONNECTED_PROVIDERS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(providers)
    }

    pub async fn preferred_provider_id(&self) -> Result<Option<ProviderId>> {
        sqlx::query_scalar(queries::PREFERRED_PROVIDER_QUERY)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn prefer_model_config(&self, provider_id: &ProviderId) -> Result<PreferModelConfig> {
        sqlx::query_as::<_, PreferModelConfig>(queries::PREFER_MODEL_CONFIG_QUERY)
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StorageError::NotFound(provider_id.to_string()))
    }

    pub async fn insert_plugin(
        &self,
        name: &str,
        plugin_type: PluginType,
        transport: Transport,
        timeout: i64,
        env: &HashMap<String, String>,
        args: &PluginArgs,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_PLUGIN_QUERY)
            .bind(name)
            .bind(plugin_type)
            .bind(transport)
            .bind(timeout)
            .bind(serde_json::to_string(env)?)
            .bind(serde_json::to_string(args)?)
            .execute(&self.pool)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => {
                    StorageError::Duplicate(name.to_string())
                },
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn update_plugin(
        &self,
        name: &str,
        transport: Transport,
        timeout: i64,
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
            return Err(StorageError::NotFound(name.to_string()));
        }
        Ok(())
    }

    pub async fn all_mcp_plugins(&self) -> Result<Vec<Plugin>> {
        let plugins = sqlx::query_as::<_, Plugin>(queries::GET_ALL_MCP_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(plugins)
    }

    pub async fn delete_plugin(&self, name: &str) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PLUGIN_QUERY)
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(name.to_string()));
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
            return Err(StorageError::NotFound(name.to_string()));
        }
        Ok(())
    }

    pub async fn disabled_plugins(&self) -> Result<HashSet<String>> {
        let names = sqlx::query_scalar::<_, String>(queries::DISABLED_PLUGINS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(names.into_iter().collect())
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

    pub async fn insert_history(
        &self,
        session_id: &str,
        payload_type: EntryType,
        payload: &Value,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_HISTORY)
            .bind(session_id)
            .bind(payload_type)
            .bind(serde_json::to_string(payload)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_history(&self, session_id: &str) -> Result<Vec<FileEntry>> {
        let entries = sqlx::query_as::<_, FileEntry>(queries::GET_HISTORY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(entries)
    }

    pub async fn restore_history(&self, session_id: &str) -> Result<Vec<RestoreEntry>> {
        let entries = sqlx::query_as::<_, RestoreEntry>(queries::RESTORE_HISTORY)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(entries)
    }

    /// Prune partially-written turns left by a crash or cold start: for every
    /// session whose newest history item isn't a completed assistant message,
    /// drop everything back to (and including) the last user prompt.
    pub async fn recover_history(&self) -> Result<()> {
        sqlx::query(queries::RECOVER).execute(&self.pool).await?;
        Ok(())
    }

    /// Roll one session back to its last completed assistant message, dropping
    /// everything after it. Used to clean up a failed turn or on user request.
    pub async fn rollback_history(&self, session_id: &str) -> Result<()> {
        sqlx::query(queries::ROLLBACK)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a session (cascading its history) when it holds no completed
    /// assistant message. Returns whether a session was removed.
    pub async fn delete_empty_session(&self, session_id: &str) -> Result<bool> {
        let result = sqlx::query(queries::DELETE_EMPTY_SESSION)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
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

    #[error("provider {0} not found")]
    NotFound(String),

    #[error("provider {0} already exists")]
    Duplicate(String),
}

type Result<T> = std::result::Result<T, StorageError>;

#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
