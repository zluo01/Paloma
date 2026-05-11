use crate::error::{Result, StorageError};
use crate::queries;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use std::path::Path;
use std::time::Duration;

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
        expires_at: Option<i64>,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        sqlx::query(queries::INSERT_PROVIDER_QUERY)
            .bind(provider_id)
            .bind(auth_kind)
            .bind(secret)
            .bind(expires_at)
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
        expires_at: Option<i64>,
    ) -> Result<()> {
        let result = sqlx::query(queries::UPDATE_PROVIDER_QUERY)
            .bind(auth_kind)
            .bind(secret)
            .bind(expires_at)
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

    pub async fn connected_providers(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(queries::CONNECTED_PROVIDERS_QUERY)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("provider_id"))
            .collect())
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
                None,
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
                Some(1_700_000_000),
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .expect("insert");

        let row = sqlx::query("SELECT provider_id, auth_kind, secret, expires_at FROM provider_credentials WHERE provider_id = ?")
            .bind("anthropic")
            .fetch_one(storage.pool())
            .await
            .unwrap();

        assert_eq!(row.get::<String, _>("provider_id"), "anthropic");
        assert_eq!(row.get::<String, _>("auth_kind"), "api_key");
        assert_eq!(row.get::<String, _>("secret"), "sk-abc");
        assert_eq!(row.get::<Option<i64>, _>("expires_at"), Some(1_700_000_000));
    }

    #[tokio::test]
    async fn insert_provider_duplicate_returns_duplicate_error() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider("codex", "oauth", "tok-1", None, "gpt-5", "medium")
            .await
            .expect("first insert");

        let err = storage
            .insert_provider("codex", "oauth", "tok-2", None, "gpt-5", "medium")
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
            .insert_provider("openai", "magic_link", "x", None, "gpt-5", "medium")
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
            .insert_provider("codex", "oauth", "old-token", Some(100), "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .update_provider("codex", "oauth", "new-token", Some(999))
            .await
            .expect("update");

        let row = sqlx::query(
            "SELECT secret, expires_at FROM provider_credentials WHERE provider_id = ?",
        )
        .bind("codex")
        .fetch_one(storage.pool())
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("secret"), "new-token");
        assert_eq!(row.get::<Option<i64>, _>("expires_at"), Some(999));
    }

    #[tokio::test]
    async fn update_provider_nonexistent_returns_not_found() {
        let (storage, _tmp) = fresh_storage().await;
        let err = storage
            .update_provider("ghost", "api_key", "x", None)
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
            .insert_provider(
                "anthropic",
                "api_key",
                "x",
                None,
                "claude-sonnet-4-5",
                "medium",
            )
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
        let ids = storage.connected_providers().await.expect("query");
        assert!(ids.is_empty(), "expected empty, got {ids:?}");
    }

    #[tokio::test]
    async fn connected_providers_returns_all_inserted_ids() {
        let (storage, _tmp) = fresh_storage().await;
        storage
            .insert_provider(
                "anthropic",
                "api_key",
                "sk-a",
                None,
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider("codex", "oauth", "tok", Some(123), "gpt-5", "medium")
            .await
            .unwrap();

        let mut ids = storage.connected_providers().await.expect("query");
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
                None,
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();
        storage
            .insert_provider("codex", "oauth", "tok", None, "gpt-5", "medium")
            .await
            .unwrap();

        storage.delete_provider("anthropic").await.unwrap();

        let ids = storage.connected_providers().await.expect("query");
        assert_eq!(ids, vec!["codex".to_string()]);
    }
}
