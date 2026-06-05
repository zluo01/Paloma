use super::queries;
use crate::error::{Result, StorageError};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{FromRow, Pool, Sqlite};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct Storage {
    pool: Pool<Sqlite>,
}

impl Storage {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let pool = create_pool(db_path).await?;
        initialize(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn insert_provider(
        &self,
        provider_id: &str,
        auth_kind: &str,
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
                }
                _ => e.into(),
            })?;
        Ok(())
    }

    pub async fn update_provider(
        &self,
        provider_id: &str,
        auth_kind: &str,
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
        provider_id: &str,
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

    pub async fn delete_provider(&self, provider_id: &str) -> Result<()> {
        let result = sqlx::query(queries::DELETE_PROVIDER_QUERY)
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(provider_id.to_string()));
        }
        Ok(())
    }

    pub async fn create_new_session(
        &self,
        session_id: Uuid,
        provider_id: &str,
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
                }
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

    pub async fn prefer_model_config(&self, provider_id: &str) -> Result<PreferModelConfig> {
        sqlx::query_as::<_, PreferModelConfig>(queries::PREFER_MODEL_CONFIG_QUERY)
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StorageError::NotFound(provider_id.to_string()))
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
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ConnectedProvider {
    pub provider_id: String,
    pub auth_kind: String,
    pub secret: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Session {
    pub session_id: String,
    pub provider_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct PreferModelConfig {
    pub model: String,
    pub effort: String,
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
    use super::*;
    use sqlx::Row;
    use tempfile::TempDir;

    /// Spin up a `Storage` backed by a fresh file in a tempdir.
    /// Returns the `TempDir` guard so the directory survives the test.
    async fn fresh_storage() -> (Storage, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let storage = Storage::new(&tmp.path().join("scry.db"))
            .await
            .expect("Storage::new");
        (storage, tmp)
    }

    #[tokio::test]
    async fn new_creates_database() {
        let (storage, _tmp) = fresh_storage().await;
        // Sanity: the table exists and is empty.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_credentials")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn new_is_idempotent_across_reopens() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("scry.db");

        let first = Storage::new(&path).await.expect("first open");
        first
            .insert_provider(
                "anthropic",
                "api_key",
                "sk-1",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        drop(first);

        // Second open must succeed (regression for missing IF NOT EXISTS)
        // and must see the previously inserted row.
        let second = Storage::new(&path).await.expect("second open");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_credentials")
            .fetch_one(second.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn insert_provider_writes_row() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider(
                "anthropic",
                "api_key",
                "sk-abc",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .expect("insert");

        let row = sqlx::query(
            "SELECT provider_id, auth_kind, secret FROM provider_credentials WHERE provider_id = ?",
        )
        .bind("anthropic")
        .fetch_one(storage.pool())
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("provider_id"), "anthropic");
        assert_eq!(row.get::<String, _>("auth_kind"), "api_key");
        assert_eq!(row.get::<String, _>("secret"), "sk-abc");
    }

    #[tokio::test]
    async fn insert_provider_duplicate_returns_duplicate_error() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok-1", "gpt-5", "medium")
            .await
            .expect("first insert");

        let err = storage
            .insert_provider("codex", "oauth", "tok-2", "gpt-5", "medium")
            .await
            .expect_err("second insert must fail");

        assert!(
            matches!(err, StorageError::Duplicate(ref id) if id == "codex"),
            "expected Duplicate(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn insert_provider_rejects_bad_auth_kind() {
        let (storage, _tmp) = fresh_storage().await;
        let err = storage
            .insert_provider("openai", "magic_link", "x", "gpt-5", "medium")
            .await
            .expect_err("CHECK constraint should reject unknown auth_kind");

        // A CHECK violation is a database error, not a unique violation,
        // so it surfaces as `Sqlx`, not `Duplicate`.
        assert!(matches!(err, StorageError::Sqlx(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn update_provider_changes_row() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "old-token", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .update_provider("codex", "oauth", "new-token")
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
        let (storage, _tmp) = fresh_storage().await;
        let err = storage
            .update_provider("ghost", "api_key", "x")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "ghost"),
            "expected NotFound(\"ghost\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn update_preferences_changes_model_and_effort_only() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .update_preferences("codex", "gpt-5-mini", "high")
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
        let (storage, _tmp) = fresh_storage().await;
        let err = storage
            .update_preferences("ghost", "gpt-5", "medium")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "ghost"),
            "expected NotFound(\"ghost\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_provider_removes_row() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("anthropic", "api_key", "x", "claude-sonnet-4-5", "medium")
            .await
            .unwrap();

        storage.delete_provider("anthropic").await.expect("delete");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_credentials")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn delete_provider_nonexistent_returns_not_found() {
        let (storage, _tmp) = fresh_storage().await;
        let err = storage
            .delete_provider("ghost")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFound(ref id) if id == "ghost"),
            "expected NotFound(\"ghost\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn connected_providers_returns_empty_when_no_rows() {
        let (storage, _tmp) = fresh_storage().await;
        let rows = storage.connected_providers().await.expect("query");
        assert!(rows.is_empty(), "expected empty, got {rows:?}");
    }

    #[tokio::test]
    async fn connected_providers_returns_all_inserted_ids() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider(
                "anthropic",
                "api_key",
                "sk-a",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let mut ids: Vec<String> = storage
            .connected_providers()
            .await
            .expect("query")
            .into_iter()
            .map(|p| p.provider_id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["anthropic".to_string(), "codex".to_string()]);
    }

    #[tokio::test]
    async fn connected_providers_reflects_deletes() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider(
                "anthropic",
                "api_key",
                "sk-a",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        storage.delete_provider("anthropic").await.unwrap();

        let ids: Vec<String> = storage
            .connected_providers()
            .await
            .expect("query")
            .into_iter()
            .map(|p| p.provider_id)
            .collect();
        assert_eq!(ids, vec!["codex".to_string()]);
    }

    #[tokio::test]
    async fn create_session_persists_title_and_defaults() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = Uuid::parse_str("019e1234-5678-7000-8000-000000000001").unwrap();
        storage
            .create_new_session(session_id, "codex", "my first chat")
            .await
            .expect("create session");

        let sessions = storage.all_sessions().await.expect("all sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id.to_string());
        assert_eq!(sessions[0].provider_id, "codex");
        assert_eq!(sessions[0].title, "my first chat");

        // `last_update` isn't returned by `all_sessions`; read it directly to
        // confirm the insert populated it.
        let last_update: i64 =
            sqlx::query_scalar("SELECT last_update FROM sessions WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert!(last_update > 0);
    }

    #[tokio::test]
    async fn all_sessions_orders_by_last_update_latest_first() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let oldest = Uuid::parse_str("019e1234-5678-7000-8000-00000000000a").unwrap();
        let newest = Uuid::parse_str("019e1234-5678-7000-8000-00000000000b").unwrap();
        let middle = Uuid::parse_str("019e1234-5678-7000-8000-00000000000c").unwrap();
        for (id, title) in [(oldest, "oldest"), (newest, "newest"), (middle, "middle")] {
            storage
                .create_new_session(id, "codex", title)
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
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = Uuid::parse_str("019e1234-5678-7000-8000-000000000003").unwrap();
        storage
            .create_new_session(session_id, "codex", "t")
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
        let last_update: i64 =
            sqlx::query_scalar("SELECT last_update FROM sessions WHERE session_id = ?")
                .bind(session_id.to_string())
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert!(last_update > 1000, "expected bump, got {last_update}");
    }

    #[tokio::test]
    async fn touch_session_nonexistent_returns_not_found() {
        let (storage, _tmp) = fresh_storage().await;
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
    async fn unknown_command_is_not_allowed() {
        let (storage, _tmp) = fresh_storage().await;
        assert!(!storage.is_command_allowed("cargo build").await.unwrap());
    }

    #[tokio::test]
    async fn exact_permission_allows_only_the_exact_command() {
        let (storage, _tmp) = fresh_storage().await;
        storage.add_permission("git status", false).await.unwrap();

        assert!(storage.is_command_allowed("git status").await.unwrap());
        // Extra args are a different command for a non-glob entry.
        assert!(!storage.is_command_allowed("git status -s").await.unwrap());
    }

    #[tokio::test]
    async fn glob_permission_matches_on_token_boundary() {
        let (storage, _tmp) = fresh_storage().await;
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
        let (storage, _tmp) = fresh_storage().await;
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

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM permissions")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "upsert must not create a second row");
    }

    #[tokio::test]
    async fn add_permission_sets_with_glob_exactly() {
        let (storage, _tmp) = fresh_storage().await;
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
        let (storage, _tmp) = fresh_storage().await;
        storage.add_permission("cargo build", false).await.unwrap();

        // Backdate so the bump is observable regardless of clock resolution.
        sqlx::query("UPDATE permissions SET updated_at = 1000 WHERE prefix = ?")
            .bind("cargo build")
            .execute(storage.pool())
            .await
            .unwrap();

        storage.add_permission("cargo build", true).await.unwrap();

        let updated_at: i64 =
            sqlx::query_scalar("SELECT updated_at FROM permissions WHERE prefix = ?")
                .bind("cargo build")
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert!(updated_at > 1000, "expected refresh, got {updated_at}");
    }
}
