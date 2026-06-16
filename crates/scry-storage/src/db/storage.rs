use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Pool, Sqlite,
};
use uuid::Uuid;

use super::queries;
use crate::{
    db::entity::{
        ConnectedProvider, EntryType, FileEntry, Plugin, PluginArgs, PluginType, PreferModelConfig,
        ProviderId, RestoreEntry, Session, Transport,
    },
    error::{Result, StorageError},
    AuthKind,
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

    pub fn pool(&self) -> &Pool<Sqlite> {
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
                    StorageError::Duplicate(provider_id.as_str().to_owned())
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
            return Err(StorageError::NotFound(provider_id.as_str().to_owned()));
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
            return Err(StorageError::NotFound(provider_id.as_str().to_owned()));
        }
        Ok(())
    }

    pub async fn delete_provider(&self, provider_id: &ProviderId) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PROVIDER_QUERY)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(provider_id.as_str().to_owned()));
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

    pub async fn prefer_model_config(&self, provider_id: &ProviderId) -> Result<PreferModelConfig> {
        sqlx::query_as::<_, PreferModelConfig>(queries::PREFER_MODEL_CONFIG_QUERY)
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StorageError::NotFound(provider_id.as_str().to_owned()))
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::Row;

    use super::*;

    async fn fresh_storage() -> Storage {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(":memory:"))
            .await
            .expect("in-memory pool");
        Storage::from_pool(pool).await.expect("Storage::from_pool")
    }

    #[tokio::test]
    async fn new_creates_database() {
        let storage = fresh_storage().await;
        let providers = storage.connected_providers().await.expect("providers");
        assert!(providers.is_empty(), "expected empty, got {providers:?}");
    }

    #[tokio::test]
    async fn new_is_idempotent_across_reopens() {
        let uri = "file:scry_reopen_idempotent?mode=memory&cache=shared";
        let keepalive = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(uri)
            .await
            .expect("keepalive pool");

        let first = Storage::from_pool(
            SqlitePoolOptions::new()
                .connect(uri)
                .await
                .expect("first pool"),
        )
        .await
        .expect("first open");
        first
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::ApiKey,
                "sk-1",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        drop(first);

        // Second open must succeed (regression for missing IF NOT EXISTS)
        // and must see the previously inserted row.
        let second = Storage::from_pool(
            SqlitePoolOptions::new()
                .connect(uri)
                .await
                .expect("second pool"),
        )
        .await
        .expect("second open");
        let providers = second.connected_providers().await.expect("providers");
        assert_eq!(providers.len(), 1);

        drop(keepalive);
    }

    #[tokio::test]
    async fn insert_provider_writes_row() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::ApiKey,
                "sk-abc",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .expect("insert");

        let row = sqlx::query(
            "SELECT provider_id, auth_kind, secret FROM provider_credentials WHERE provider_id = ?",
        )
        .bind("codex")
        .fetch_one(storage.pool())
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("provider_id"), "codex");
        assert_eq!(row.get::<String, _>("auth_kind"), "api_key");
        assert_eq!(row.get::<String, _>("secret"), "sk-abc");
    }

    #[tokio::test]
    async fn insert_provider_duplicate_returns_duplicate_error() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok-1",
                "gpt-5",
                "medium",
            )
            .await
            .expect("first insert");

        let err = storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok-2",
                "gpt-5",
                "medium",
            )
            .await
            .expect_err("second insert must fail");

        assert!(
            matches!(err, StorageError::Duplicate(ref id) if id == "codex"),
            "expected Duplicate(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn update_provider_changes_row() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "old-token",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .update_provider(&ProviderId::Codex, &AuthKind::Oauth, "new-token")
            .await
            .expect("update");

        let row = sqlx::query("SELECT secret FROM provider_credentials WHERE provider_id = ?")
            .bind("codex")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("secret"), "new-token");
    }

    #[tokio::test]
    async fn update_provider_nonexistent_returns_not_found() {
        let storage = fresh_storage().await;
        let err = storage
            .update_provider(&ProviderId::Codex, &AuthKind::ApiKey, "x")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "codex"),
            "expected NotFound(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn update_preferences_changes_model_and_effort_only() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .update_preferences(&ProviderId::Codex, "gpt-5-mini", "high")
            .await
            .expect("update prefs");

        let row = sqlx::query(
            "SELECT secret, model, effort FROM provider_credentials WHERE provider_id = ?",
        )
        .bind("codex")
        .fetch_one(storage.pool())
        .await
        .unwrap();

        // Auth fields untouched.
        assert_eq!(row.get::<String, _>("secret"), "tok");
        // Preferences updated.
        assert_eq!(row.get::<String, _>("model"), "gpt-5-mini");
        assert_eq!(row.get::<String, _>("effort"), "high");
    }

    #[tokio::test]
    async fn update_preferences_nonexistent_returns_not_found() {
        let storage = fresh_storage().await;
        let err = storage
            .update_preferences(&ProviderId::Codex, "gpt-5", "medium")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "codex"),
            "expected NotFound(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_provider_removes_row() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::ApiKey,
                "x",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .delete_provider(&ProviderId::Codex)
            .await
            .expect("delete");

        let providers = storage.connected_providers().await.expect("providers");
        assert!(providers.is_empty(), "expected empty, got {providers:?}");
    }

    #[tokio::test]
    async fn delete_provider_nonexistent_returns_not_found() {
        let storage = fresh_storage().await;
        let err = storage
            .delete_provider(&ProviderId::Codex)
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "codex"),
            "expected NotFound(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_provider_nonexistent_keeps_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let err = storage
            .delete_provider(&ProviderId::Anthropic)
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "anthropic"),
            "expected NotFound(\"anthropic\"), got {err:?}",
        );
        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::Codex]);

        let providers = storage.connected_providers().await.expect("providers");
        assert_eq!(providers.len(), 2);
    }

    #[tokio::test]
    async fn connected_providers_returns_empty_when_no_rows() {
        let storage = fresh_storage().await;
        let rows = storage.connected_providers().await.expect("query");
        assert!(rows.is_empty(), "expected empty, got {rows:?}");
    }

    #[tokio::test]
    async fn connected_providers_returns_all_inserted_ids() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let ids: Vec<ProviderId> = storage
            .connected_providers()
            .await
            .expect("query")
            .into_iter()
            .map(|p| p.provider_id)
            .collect();
        assert_eq!(ids, vec![ProviderId::Codex]);
    }

    /// The providers currently flagged preferred — at most one under the
    /// insert / `set_preferred` invariant.
    async fn preferred_ids(storage: &Storage) -> Vec<ProviderId> {
        storage
            .connected_providers()
            .await
            .expect("connected providers")
            .into_iter()
            .filter(|p| p.preferred)
            .map(|p| p.provider_id)
            .collect()
    }

    #[tokio::test]
    async fn insert_provider_first_is_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::Codex]);
    }

    #[tokio::test]
    async fn insert_provider_later_is_not_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        // First connect stays preferred; a later one does not steal it.
        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::Codex]);
    }

    #[tokio::test]
    async fn set_preferred_moves_preference() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .set_preferred(&ProviderId::OpenAI)
            .await
            .expect("set preferred");

        // Exactly one preferred, and it switched to the target.
        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::OpenAI]);
    }

    #[tokio::test]
    async fn delete_preferred_provider_promotes_survivor() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .delete_provider(&ProviderId::Codex)
            .await
            .expect("delete preferred");

        // The lone survivor should inherit the preference.
        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::OpenAI]);
    }

    #[tokio::test]
    async fn delete_preferred_keeps_one_preferred_among_survivors() {
        let storage = fresh_storage().await;
        for id in [ProviderId::Codex, ProviderId::OpenAI, ProviderId::Anthropic] {
            storage
                .insert_provider(&id, &AuthKind::ApiKey, "sk", "model", "medium")
                .await
                .unwrap();
        }

        storage
            .delete_provider(&ProviderId::Codex)
            .await
            .expect("delete preferred");

        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::OpenAI]);
    }

    #[tokio::test]
    async fn delete_non_preferred_provider_keeps_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        storage
            .delete_provider(&ProviderId::OpenAI)
            .await
            .expect("delete non-preferred");

        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::Codex]);
    }

    #[tokio::test]
    async fn connect_into_empty_is_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .delete_provider(&ProviderId::Codex)
            .await
            .expect("delete last");

        storage
            .insert_provider(
                &ProviderId::OpenAI,
                &AuthKind::ApiKey,
                "sk",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        assert_eq!(preferred_ids(&storage).await, vec![ProviderId::OpenAI]);
    }

    #[tokio::test]
    async fn create_session_persists_title_and_defaults() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let session_id = Uuid::parse_str("019e1234-5678-7000-8000-000000000001").unwrap();
        storage
            .create_new_session(session_id, &ProviderId::Codex, "my first chat")
            .await
            .expect("create session");

        let sessions = storage.all_sessions().await.expect("all sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id.to_string());
        assert_eq!(sessions[0].provider_id, "codex");
        assert_eq!(sessions[0].title, "my first chat");

        // `last_update` isn't returned by `all_sessions`; read it directly to
        // confirm the insert populated it.
        let row = sqlx::query("SELECT last_update FROM sessions WHERE session_id = ?")
            .bind(session_id.to_string())
            .fetch_one(storage.pool())
            .await
            .unwrap();
        let last_update = row.get::<i64, _>("last_update");
        assert!(last_update > 0);
    }

    #[tokio::test]
    async fn all_sessions_orders_by_last_update_latest_first() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let oldest = Uuid::parse_str("019e1234-5678-7000-8000-00000000000a").unwrap();
        let newest = Uuid::parse_str("019e1234-5678-7000-8000-00000000000b").unwrap();
        let middle = Uuid::parse_str("019e1234-5678-7000-8000-00000000000c").unwrap();
        for (id, title) in [(oldest, "oldest"), (newest, "newest"), (middle, "middle")] {
            storage
                .create_new_session(id, &ProviderId::Codex, title)
                .await
                .unwrap();
        }

        // Force distinct last_update values regardless of insertion clock.
        for (id, ts) in [(oldest, 100_i64), (newest, 300), (middle, 200)] {
            sqlx::query("UPDATE sessions SET last_update = ? WHERE session_id = ?")
                .bind(ts)
                .bind(id.to_string())
                .execute(storage.pool())
                .await
                .unwrap();
        }

        let titles: Vec<String> = storage
            .all_sessions()
            .await
            .expect("all sessions")
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, vec!["newest", "middle", "oldest"]);
    }

    #[tokio::test]
    async fn touch_session_bumps_last_update() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let session_id = Uuid::parse_str("019e1234-5678-7000-8000-000000000003").unwrap();
        storage
            .create_new_session(session_id, &ProviderId::Codex, "t")
            .await
            .unwrap();

        // Backdate so the bump is observable regardless of clock resolution.
        sqlx::query("UPDATE sessions SET last_update = 1000 WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(storage.pool())
            .await
            .unwrap();

        storage
            .touch_session(&session_id.to_string())
            .await
            .expect("touch");

        // `last_update` isn't returned by `all_sessions`; read it directly.
        let row = sqlx::query("SELECT last_update FROM sessions WHERE session_id = ?")
            .bind(session_id.to_string())
            .fetch_one(storage.pool())
            .await
            .unwrap();
        let last_update = row.get::<i64, _>("last_update");
        assert!(last_update > 1000, "expected bump, got {last_update}");
    }

    #[tokio::test]
    async fn touch_session_nonexistent_returns_not_found() {
        let storage = fresh_storage().await;
        let err = storage
            .touch_session("019e1234-5678-7000-8000-0000000000fe")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "019e1234-5678-7000-8000-0000000000fe"),
            "expected NotFound, got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_session_cascades_to_history() {
        let storage = fresh_storage().await;
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();

        let session_id = Uuid::parse_str("019e1234-5678-7000-8000-000000000004").unwrap();
        storage
            .create_new_session(session_id, &ProviderId::Codex, "with history")
            .await
            .unwrap();

        let session_id = session_id.to_string();
        storage
            .insert_history(
                &session_id,
                EntryType::ResponseItem,
                &json!({"type": "message", "text": "hello"}),
            )
            .await
            .unwrap();
        storage
            .insert_history(
                &session_id,
                EntryType::EventMsg,
                &json!({"type": "event", "name": "done"}),
            )
            .await
            .unwrap();

        assert_eq!(storage.get_history(&session_id).await.unwrap().len(), 2);

        storage
            .delete_session(&session_id)
            .await
            .expect("delete session");

        assert!(storage
            .all_sessions()
            .await
            .unwrap()
            .into_iter()
            .all(|session| session.session_id != session_id));
        assert!(storage.get_history(&session_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_command_is_not_allowed() {
        let storage = fresh_storage().await;
        assert!(!storage.is_command_allowed("cargo build").await.unwrap());
    }

    #[tokio::test]
    async fn exact_permission_allows_only_the_exact_command() {
        let storage = fresh_storage().await;
        storage.add_permission("git status", false).await.unwrap();

        assert!(storage.is_command_allowed("git status").await.unwrap());
        // Extra args are a different command for a non-glob entry.
        assert!(!storage.is_command_allowed("git status -s").await.unwrap());
    }

    #[tokio::test]
    async fn glob_permission_matches_on_token_boundary() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", true).await.unwrap();

        // Exact prefix and any space-separated continuation are allowed.
        assert!(storage.is_command_allowed("cargo build").await.unwrap());
        assert!(storage
            .is_command_allowed("cargo build -j 8")
            .await
            .unwrap());
        // A different binary that merely shares a leading substring must not.
        assert!(!storage
            .is_command_allowed("cargo buildkit run")
            .await
            .unwrap());
        // Shorter than the prefix never matches.
        assert!(!storage.is_command_allowed("cargo").await.unwrap());
    }

    #[tokio::test]
    async fn add_permission_widens_exact_to_glob_in_place() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", false).await.unwrap();
        assert!(!storage
            .is_command_allowed("cargo build -j 8")
            .await
            .unwrap());

        // Re-approving as a glob upserts the same row and widens it.
        storage.add_permission("cargo build", true).await.unwrap();
        assert!(storage
            .is_command_allowed("cargo build -j 8")
            .await
            .unwrap());

        let rows = sqlx::query("SELECT prefix FROM permissions")
            .fetch_all(storage.pool())
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "upsert must not create a second row");
    }

    #[tokio::test]
    async fn add_permission_sets_with_glob_exactly() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", true).await.unwrap();
        assert!(storage
            .is_command_allowed("cargo build -j 8")
            .await
            .unwrap());

        // A later exact re-approval narrows the row (last-writer-wins).
        storage.add_permission("cargo build", false).await.unwrap();
        assert!(storage.is_command_allowed("cargo build").await.unwrap());
        assert!(!storage
            .is_command_allowed("cargo build -j 8")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn add_permission_refreshes_updated_at_on_conflict() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", false).await.unwrap();

        // Backdate so the bump is observable regardless of clock resolution.
        sqlx::query("UPDATE permissions SET updated_at = 1000 WHERE prefix = ?")
            .bind("cargo build")
            .execute(storage.pool())
            .await
            .unwrap();

        storage.add_permission("cargo build", true).await.unwrap();

        let row = sqlx::query("SELECT updated_at FROM permissions WHERE prefix = ?")
            .bind("cargo build")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        let updated_at = row.get::<i64, _>("updated_at");
        assert!(updated_at > 1000, "expected refresh, got {updated_at}");
    }

    // ---- recover ----

    async fn seed_provider(storage: &Storage) {
        storage
            .insert_provider(
                &ProviderId::Codex,
                &AuthKind::Oauth,
                "tok",
                "gpt-5",
                "medium",
            )
            .await
            .unwrap();
    }

    async fn seed_session(storage: &Storage, id: Uuid, items: &[Value]) {
        storage
            .create_new_session(id, &ProviderId::Codex, "s")
            .await
            .unwrap();
        for item in items {
            storage
                .insert_history(&id.to_string(), EntryType::ResponseItem, item)
                .await
                .unwrap();
        }
    }

    async fn history_len(storage: &Storage, id: Uuid) -> usize {
        storage.get_history(&id.to_string()).await.unwrap().len()
    }

    fn user() -> Value {
        json!({ "type": "message", "role": "user" })
    }
    fn assistant_done() -> Value {
        json!({ "type": "message", "role": "assistant", "status": "completed" })
    }
    fn reasoning() -> Value {
        json!({ "type": "reasoning" })
    }
    fn function_call() -> Value {
        json!({ "type": "function_call", "call_id": "c1" })
    }

    #[tokio::test]
    async fn recover_prunes_unfinished_turn_back_to_last_prompt() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a0").unwrap();
        seed_session(
            &storage,
            id,
            &[
                user(),
                assistant_done(),
                user(),
                reasoning(),
                function_call(),
            ],
        )
        .await;

        storage.recover_history().await.unwrap();

        let history = storage.get_history(&id.to_string()).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history.last().unwrap().payload["status"], "completed");
    }

    #[tokio::test]
    async fn recover_keeps_session_ending_in_completed_message() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a1").unwrap();
        seed_session(&storage, id, &[user(), assistant_done()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 2);
    }

    #[tokio::test]
    async fn recover_empties_session_with_no_completed_turn() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a2").unwrap();
        seed_session(&storage, id, &[user(), reasoning()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 0);
    }

    #[tokio::test]
    async fn recover_prunes_dangling_user_prompt() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a3").unwrap();
        // A trailing user prompt has no `status`, so it isn't a completed message.
        seed_session(&storage, id, &[user(), assistant_done(), user()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 2);
    }

    #[tokio::test]
    async fn recover_only_touches_unfinished_sessions() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;

        // Two finished sessions (one single-turn, one multi-turn) stay intact;
        // three unfinished sessions with different broken tails are each pruned
        // back to their last completed turn in a single recover pass.
        let finished_single = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a4").unwrap();
        let finished_multi = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a5").unwrap();
        let dropped_tool_call = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a6").unwrap();
        let no_completed_turn = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a7").unwrap();
        let dangling_prompt = Uuid::parse_str("019e1234-5678-7000-8000-0000000000a8").unwrap();

        seed_session(&storage, finished_single, &[user(), assistant_done()]).await;
        seed_session(
            &storage,
            finished_multi,
            &[user(), assistant_done(), user(), assistant_done()],
        )
        .await;
        seed_session(
            &storage,
            dropped_tool_call,
            &[
                user(),
                assistant_done(),
                user(),
                reasoning(),
                function_call(),
            ],
        )
        .await;
        seed_session(&storage, no_completed_turn, &[user(), reasoning()]).await;
        seed_session(
            &storage,
            dangling_prompt,
            &[user(), assistant_done(), user()],
        )
        .await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, finished_single).await, 2);
        assert_eq!(history_len(&storage, finished_multi).await, 4);
        assert_eq!(history_len(&storage, dropped_tool_call).await, 2);
        assert_eq!(history_len(&storage, no_completed_turn).await, 0);
        assert_eq!(history_len(&storage, dangling_prompt).await, 2);
    }

    // ---- rollback ----

    #[tokio::test]
    async fn rollback_drops_items_after_last_completed_message() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000b0").unwrap();
        seed_session(
            &storage,
            id,
            &[
                user(),
                assistant_done(),
                user(),
                reasoning(),
                function_call(),
            ],
        )
        .await;

        storage.rollback_history(&id.to_string()).await.unwrap();

        let history = storage.get_history(&id.to_string()).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history.last().unwrap().payload["status"], "completed");
    }

    #[tokio::test]
    async fn rollback_empties_session_without_completed_message() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = Uuid::parse_str("019e1234-5678-7000-8000-0000000000b2").unwrap();
        seed_session(&storage, id, &[user(), reasoning()]).await;

        storage.rollback_history(&id.to_string()).await.unwrap();

        assert_eq!(history_len(&storage, id).await, 0);
    }

    #[tokio::test]
    async fn rollback_only_affects_target_session() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let target = Uuid::parse_str("019e1234-5678-7000-8000-0000000000b3").unwrap();
        let other = Uuid::parse_str("019e1234-5678-7000-8000-0000000000b4").unwrap();
        seed_session(
            &storage,
            target,
            &[user(), assistant_done(), user(), reasoning()],
        )
        .await;
        seed_session(
            &storage,
            other,
            &[user(), assistant_done(), user(), reasoning()],
        )
        .await;

        storage.rollback_history(&target.to_string()).await.unwrap();

        assert_eq!(history_len(&storage, target).await, 2);
        assert_eq!(history_len(&storage, other).await, 4);
    }
}
