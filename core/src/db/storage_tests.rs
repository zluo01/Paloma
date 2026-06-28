use serde_json::json;
use sqlx::Row;

use super::*;
use crate::provider::ConversationItem;

async fn fresh_storage() -> Storage {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(":memory:"))
        .await
        .expect("in-memory pool");
    Storage::from_pool(pool).await.expect("Storage::from_pool")
}

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}

mod storage {
    use super::*;

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
}

mod providers {
    use super::*;

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
            matches!(err, StorageError::Duplicate(ref id) if id == &ProviderId::Codex.to_string()),
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
            matches!(err, StorageError::NotFound(ref id) if id == &ProviderId::Codex.to_string()),
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
            matches!(err, StorageError::NotFound(ref id) if id == &ProviderId::Codex.to_string()),
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
            matches!(err, StorageError::NotFound(ref id) if id ==&ProviderId::Codex.to_string()),
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
            matches!(err, StorageError::NotFound(ref id) if id == &ProviderId::Anthropic.to_string()),
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
    async fn preferred_provider_returns_current_preferred() {
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
                "gpt-5-mini",
                "high",
            )
            .await
            .unwrap();

        storage
            .set_preferred(&ProviderId::OpenAI)
            .await
            .expect("set preferred");

        let provider_id = storage
            .preferred_provider_id()
            .await
            .expect("preferred provider");
        assert_eq!(provider_id, Some(ProviderId::OpenAI));
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
}

mod sessions {
    use super::*;

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

        let session_id = uuid("019e1234-5678-7000-8000-000000000001");
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

        let oldest = uuid("019e1234-5678-7000-8000-00000000000a");
        let newest = uuid("019e1234-5678-7000-8000-00000000000b");
        let middle = uuid("019e1234-5678-7000-8000-00000000000c");
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

        let session_id = uuid("019e1234-5678-7000-8000-000000000003");
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

        let session_id = uuid("019e1234-5678-7000-8000-000000000004");
        storage
            .create_new_session(session_id, &ProviderId::Codex, "with history")
            .await
            .unwrap();

        let session_id = session_id.to_string();
        storage
            .insert_history(
                &session_id,
                &ProviderId::Codex,
                &ConversationItem::Message {
                    message: vec![],
                    provider_meta: Default::default(),
                },
            )
            .await
            .unwrap();
        storage
            .insert_history(
                &session_id,
                &ProviderId::Codex,
                &ConversationItem::Reasoning {
                    reasoning: vec![],
                    provider_meta: Default::default(),
                },
            )
            .await
            .unwrap();

        assert_eq!(storage.get_history(&session_id).await.unwrap().len(), 2);

        storage
            .delete_session(&session_id)
            .await
            .expect("delete session");

        assert!(
            storage
                .all_sessions()
                .await
                .unwrap()
                .into_iter()
                .all(|session| session.session_id != session_id)
        );
        assert!(storage.get_history(&session_id).await.unwrap().is_empty());
    }
}

mod history {
    use super::*;

    #[tokio::test]
    async fn history_round_trips_every_conversation_item_payload() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;

        let session_id = uuid("019e1234-5678-7000-8000-000000000005");
        storage
            .create_new_session(session_id, &ProviderId::Codex, "round trip")
            .await
            .unwrap();

        let items = every_conversation_item();

        for item in &items {
            storage
                .insert_history(&session_id.to_string(), &ProviderId::Codex, item)
                .await
                .unwrap();
        }

        let history = storage.get_history(&session_id.to_string()).await.unwrap();
        let payloads = history
            .iter()
            .map(|entry| entry.payload.clone())
            .collect::<Vec<_>>();

        assert_eq!(payloads, items);
        assert!(
            history
                .iter()
                .all(|entry| entry.provider_id == ProviderId::Codex)
        );
    }

    #[tokio::test]
    async fn history_preserves_provider_id_per_row() {
        let storage = fresh_storage().await;
        seed_provider_id(&storage, &ProviderId::Codex).await;
        seed_provider_id(&storage, &ProviderId::OpenAI).await;

        let session_id = uuid("019e1234-5678-7000-8000-000000000006");
        storage
            .create_new_session(session_id, &ProviderId::Codex, "mixed providers")
            .await
            .unwrap();

        let user = user();
        let message = assistant_message();
        storage
            .insert_history(&session_id.to_string(), &ProviderId::Codex, &user)
            .await
            .unwrap();
        storage
            .insert_history(&session_id.to_string(), &ProviderId::OpenAI, &message)
            .await
            .unwrap();

        let history = storage.get_history(&session_id.to_string()).await.unwrap();

        assert_eq!(
            history
                .iter()
                .map(|entry| entry.provider_id)
                .collect::<Vec<_>>(),
            vec![ProviderId::Codex, ProviderId::OpenAI]
        );
        assert_eq!(
            history
                .into_iter()
                .map(|entry| entry.payload)
                .collect::<Vec<_>>(),
            vec![user, message]
        );
    }

    #[tokio::test]
    async fn get_history_rejects_invalid_conversation_item_payload() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let session_id = uuid("019e1234-5678-7000-8000-000000000007");
        seed_session(&storage, session_id, &[]).await;

        insert_invalid_history_payload(&storage, &session_id.to_string()).await;

        let err = storage
            .get_history(&session_id.to_string())
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Sqlx(_)), "{err:?}");
    }

    #[tokio::test]
    async fn restore_history_rejects_invalid_conversation_item_payload() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let session_id = uuid("019e1234-5678-7000-8000-000000000008");
        seed_session(&storage, session_id, &[]).await;

        insert_invalid_history_payload(&storage, &session_id.to_string()).await;

        let err = storage
            .restore_history(&session_id.to_string())
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Sqlx(_)), "{err:?}");
    }

    #[tokio::test]
    async fn restore_history_excludes_tool_results_and_marks_finished_tool_calls() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        seed_provider_id(&storage, &ProviderId::OpenAI).await;
        let session_id = uuid("019e1234-5678-7000-8000-000000000009");
        let pending_tool_call = function_call_with("pending_call");
        let finished_tool_call = function_call_with("finished_call");
        let tool_result = tool_result_with("finished_call");
        let hosted_tool = hosted_tool();
        let message = assistant_message();
        storage
            .create_new_session(session_id, &ProviderId::Codex, "restore")
            .await
            .unwrap();
        for (provider_id, item) in [
            (ProviderId::Codex, user()),
            (ProviderId::OpenAI, pending_tool_call.clone()),
            (ProviderId::Codex, reasoning()),
            (ProviderId::OpenAI, finished_tool_call.clone()),
            (ProviderId::OpenAI, tool_result),
            (ProviderId::Codex, hosted_tool.clone()),
            (ProviderId::OpenAI, message.clone()),
        ] {
            storage
                .insert_history(&session_id.to_string(), &provider_id, &item)
                .await
                .unwrap();
        }

        let restored = storage
            .restore_history(&session_id.to_string())
            .await
            .unwrap();

        assert_eq!(
            restored
                .iter()
                .map(|entry| entry.payload.clone())
                .collect::<Vec<_>>(),
            vec![
                user(),
                pending_tool_call,
                reasoning(),
                finished_tool_call,
                hosted_tool,
                message,
            ]
        );
        assert_eq!(
            restored
                .iter()
                .map(|entry| entry.finished)
                .collect::<Vec<_>>(),
            vec![false, false, false, true, false, false]
        );
        assert_eq!(
            restored
                .iter()
                .map(|entry| entry.provider_id)
                .collect::<Vec<_>>(),
            vec![
                ProviderId::Codex,
                ProviderId::OpenAI,
                ProviderId::Codex,
                ProviderId::OpenAI,
                ProviderId::Codex,
                ProviderId::OpenAI,
            ]
        );
    }

    // ---- history ----

    async fn seed_provider(storage: &Storage) {
        seed_provider_id(storage, &ProviderId::Codex).await;
    }

    async fn seed_provider_id(storage: &Storage, provider_id: &ProviderId) {
        storage
            .insert_provider(provider_id, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
    }

    fn conversation_item(value: serde_json::Value) -> ConversationItem {
        serde_json::from_value(value).unwrap()
    }

    fn every_conversation_item() -> Vec<ConversationItem> {
        vec![
            conversation_item(json!({
                "kind": "user_prompt",
                "prompt": "hello"
            })),
            conversation_item(json!({
                "kind": "message",
                "message": [
                    {
                        "content": "assistant text",
                        "provider_meta": {
                            "type": "output_text"
                        }
                    }
                ],
                "provider_meta": {
                    "id": "msg_123",
                    "status": "completed"
                }
            })),
            conversation_item(json!({
                "kind": "reasoning",
                "reasoning": [
                    {
                        "content": "summary text",
                        "provider_meta": {
                            "type": "summary_text"
                        }
                    }
                ],
                "provider_meta": {
                    "id": "reasoning_123",
                    "status": "completed"
                }
            })),
            conversation_item(json!({
                "kind": "tool_call",
                "call_id": "call_123",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}",
                "provider_meta": {
                    "id": "fc_123",
                    "type": "function_call"
                }
            })),
            conversation_item(json!({
                "kind": "tool_result",
                "call_id": "call_123",
                "name": "shell",
                "output": "/home/mike/Documents/_playground/gate"
            })),
            conversation_item(json!({
                "kind": "hosted_tool",
                "function_type": "web_search_call",
                "content": "searched docs",
                "provider_meta": {
                    "id": "ws_123",
                    "status": "completed"
                }
            })),
        ]
    }

    async fn seed_session(storage: &Storage, id: Uuid, items: &[ConversationItem]) {
        storage
            .create_new_session(id, &ProviderId::Codex, "s")
            .await
            .unwrap();
        for item in items {
            storage
                .insert_history(&id.to_string(), &ProviderId::Codex, item)
                .await
                .unwrap();
        }
    }

    async fn history_len(storage: &Storage, id: Uuid) -> usize {
        storage.get_history(&id.to_string()).await.unwrap().len()
    }

    fn user() -> ConversationItem {
        ConversationItem::UserPrompt {
            prompt: "example prompt".to_string(),
        }
    }

    fn assistant_message() -> ConversationItem {
        ConversationItem::Message {
            message: vec![],
            provider_meta: Default::default(),
        }
    }

    fn reasoning() -> ConversationItem {
        ConversationItem::Reasoning {
            reasoning: vec![],
            provider_meta: Default::default(),
        }
    }

    fn function_call() -> ConversationItem {
        function_call_with("c1")
    }

    fn function_call_with(call_id: &str) -> ConversationItem {
        ConversationItem::ToolCall {
            call_id: call_id.to_string(),
            name: "example_tool".to_string(),
            arguments: "{}".to_string(),
            provider_meta: Default::default(),
        }
    }

    fn tool_result_with(call_id: &str) -> ConversationItem {
        ConversationItem::ToolResult {
            call_id: call_id.to_string(),
            name: "example_tool".to_string(),
            output: "ok".to_string(),
        }
    }

    fn hosted_tool() -> ConversationItem {
        ConversationItem::HostedTool {
            function_type: "web_search_call".to_string(),
            content: Some("searched docs".to_string()),
            provider_meta: Default::default(),
        }
    }

    async fn insert_invalid_history_payload(storage: &Storage, session_id: &str) {
        sqlx::query(
            "INSERT INTO history (session_id, provider_id, payload_type, payload)
         VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind("codex")
        .bind("message")
        .bind(r#"{"kind":"unknown"}"#)
        .execute(storage.pool())
        .await
        .unwrap();
    }

    // ---- recover ----

    #[tokio::test]
    async fn recover_prunes_unfinished_turn_back_to_last_prompt() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000a0");
        seed_session(
            &storage,
            id,
            &[
                user(),
                assistant_message(),
                user(),
                reasoning(),
                function_call(),
            ],
        )
        .await;

        storage.recover_history().await.unwrap();

        let history = storage.get_history(&id.to_string()).await.unwrap();
        assert_eq!(history.len(), 2);
        assert!(matches!(
            &history.last().unwrap().payload,
            ConversationItem::Message { .. }
        ));
    }

    #[tokio::test]
    async fn recover_keeps_session_ending_in_completed_message() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000a1");
        seed_session(&storage, id, &[user(), assistant_message()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 2);
    }

    #[tokio::test]
    async fn recover_empties_session_with_no_completed_turn() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000a2");
        seed_session(&storage, id, &[user(), reasoning()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 0);
    }

    #[tokio::test]
    async fn recover_prunes_dangling_user_prompt() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000a3");
        seed_session(&storage, id, &[user(), assistant_message(), user()]).await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, id).await, 2);
    }

    #[tokio::test]
    async fn recover_prunes_tool_result_and_hosted_tool_tails() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let tool_result_tail = uuid("019e1234-5678-7000-8000-0000000000a9");
        let hosted_tool_tail = uuid("019e1234-5678-7000-8000-0000000000aa");

        seed_session(
            &storage,
            tool_result_tail,
            &[
                user(),
                assistant_message(),
                user(),
                tool_result_with("finished_call"),
            ],
        )
        .await;
        seed_session(
            &storage,
            hosted_tool_tail,
            &[user(), assistant_message(), user(), hosted_tool()],
        )
        .await;

        storage.recover_history().await.unwrap();

        assert_eq!(history_len(&storage, tool_result_tail).await, 2);
        assert_eq!(history_len(&storage, hosted_tool_tail).await, 2);
    }

    #[tokio::test]
    async fn recover_only_touches_unfinished_sessions() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;

        // Two finished sessions (one single-turn, one multi-turn) stay intact;
        // three unfinished sessions with different broken tails are each pruned
        // back to their last completed turn in a single recover pass.
        let finished_single = uuid("019e1234-5678-7000-8000-0000000000a4");
        let finished_multi = uuid("019e1234-5678-7000-8000-0000000000a5");
        let dropped_tool_call = uuid("019e1234-5678-7000-8000-0000000000a6");
        let no_completed_turn = uuid("019e1234-5678-7000-8000-0000000000a7");
        let dangling_prompt = uuid("019e1234-5678-7000-8000-0000000000a8");

        seed_session(&storage, finished_single, &[user(), assistant_message()]).await;
        seed_session(
            &storage,
            finished_multi,
            &[user(), assistant_message(), user(), assistant_message()],
        )
        .await;
        seed_session(
            &storage,
            dropped_tool_call,
            &[
                user(),
                assistant_message(),
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
            &[user(), assistant_message(), user()],
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
        let id = uuid("019e1234-5678-7000-8000-0000000000b0");
        seed_session(
            &storage,
            id,
            &[
                user(),
                assistant_message(),
                user(),
                reasoning(),
                function_call(),
            ],
        )
        .await;

        storage.rollback_history(&id.to_string()).await.unwrap();

        let history = storage.get_history(&id.to_string()).await.unwrap();
        assert_eq!(history.len(), 2);
        assert!(matches!(
            &history.last().unwrap().payload,
            ConversationItem::Message { .. }
        ));
    }

    #[tokio::test]
    async fn rollback_empties_session_without_completed_message() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000b2");
        seed_session(&storage, id, &[user(), reasoning()]).await;

        storage.rollback_history(&id.to_string()).await.unwrap();

        assert_eq!(history_len(&storage, id).await, 0);
    }

    #[tokio::test]
    async fn rollback_only_affects_target_session() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let target = uuid("019e1234-5678-7000-8000-0000000000b3");
        let other = uuid("019e1234-5678-7000-8000-0000000000b4");
        seed_session(
            &storage,
            target,
            &[user(), assistant_message(), user(), reasoning()],
        )
        .await;
        seed_session(
            &storage,
            other,
            &[user(), assistant_message(), user(), reasoning()],
        )
        .await;

        storage.rollback_history(&target.to_string()).await.unwrap();

        assert_eq!(history_len(&storage, target).await, 2);
        assert_eq!(history_len(&storage, other).await, 4);
    }

    // ---- delete_empty_session ----

    #[tokio::test]
    async fn delete_empty_session_removes_session_without_history() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000c0");
        seed_session(&storage, id, &[]).await;

        let removed = storage.delete_empty_session(&id.to_string()).await.unwrap();

        assert!(removed);
        assert!(
            storage
                .all_sessions()
                .await
                .unwrap()
                .iter()
                .all(|s| s.session_id != id.to_string())
        );
        assert_eq!(history_len(&storage, id).await, 0);
    }

    #[tokio::test]
    async fn delete_empty_session_keeps_session_with_history() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000c1");
        seed_session(&storage, id, &[user()]).await;

        let removed = storage.delete_empty_session(&id.to_string()).await.unwrap();

        assert!(!removed);
        assert_eq!(history_len(&storage, id).await, 1);
    }
}

mod permissions {
    use super::*;

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
        assert!(
            storage
                .is_command_allowed("cargo build -j 8")
                .await
                .unwrap()
        );
        // A different binary that merely shares a leading substring must not.
        assert!(
            !storage
                .is_command_allowed("cargo buildkit run")
                .await
                .unwrap()
        );
        // Shorter than the prefix never matches.
        assert!(!storage.is_command_allowed("cargo").await.unwrap());
    }

    #[tokio::test]
    async fn add_permission_widens_exact_to_glob_in_place() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", false).await.unwrap();
        assert!(
            !storage
                .is_command_allowed("cargo build -j 8")
                .await
                .unwrap()
        );

        // Re-approving as a glob upserts the same row and widens it.
        storage.add_permission("cargo build", true).await.unwrap();
        assert!(
            storage
                .is_command_allowed("cargo build -j 8")
                .await
                .unwrap()
        );

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
        assert!(
            storage
                .is_command_allowed("cargo build -j 8")
                .await
                .unwrap()
        );

        // A later exact re-approval narrows the row (last-writer-wins).
        storage.add_permission("cargo build", false).await.unwrap();
        assert!(storage.is_command_allowed("cargo build").await.unwrap());
        assert!(
            !storage
                .is_command_allowed("cargo build -j 8")
                .await
                .unwrap()
        );
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

    #[tokio::test]
    async fn get_permissions_returns_newest_first() {
        let storage = fresh_storage().await;
        storage.add_permission("git status", false).await.unwrap();
        storage.add_permission("cargo build", true).await.unwrap();

        sqlx::query("UPDATE permissions SET updated_at = ? WHERE prefix = ?")
            .bind(1000)
            .bind("git status")
            .execute(storage.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE permissions SET updated_at = ? WHERE prefix = ?")
            .bind(2000)
            .bind("cargo build")
            .execute(storage.pool())
            .await
            .unwrap();

        let permissions = storage.get_permissions().await.unwrap();

        assert_eq!(permissions.len(), 2);
        assert_eq!(permissions[0].prefix, "cargo build");
        assert!(permissions[0].with_glob);
        assert_eq!(permissions[0].updated_at, 2000);
        assert_eq!(permissions[1].prefix, "git status");
        assert!(!permissions[1].with_glob);
        assert_eq!(permissions[1].updated_at, 1000);
    }

    #[tokio::test]
    async fn delete_permission_removes_the_permission() {
        let storage = fresh_storage().await;
        storage.add_permission("cargo build", true).await.unwrap();

        storage.delete_permission("cargo build").await.unwrap();

        assert!(storage.get_permissions().await.unwrap().is_empty());
        assert!(!storage.is_command_allowed("cargo build").await.unwrap());
    }

    #[tokio::test]
    async fn delete_permission_reports_missing_prefix() {
        let storage = fresh_storage().await;
        let err = storage.delete_permission("cargo build").await.unwrap_err();

        assert!(matches!(err, StorageError::NotFound(ref id) if id == "cargo build"));
    }
}

mod plugins {
    use super::*;

    #[tokio::test]
    async fn insert_plugin_rejects_non_positive_timeout() {
        let storage = fresh_storage().await;
        let env = HashMap::new();
        let args = PluginArgs::Local {
            command: "echo".to_string(),
            args: vec![],
        };
        storage
            .insert_plugin("ok", PluginType::Mcp, Transport::Local, 300, &env, &args)
            .await
            .expect("positive timeout should insert");
        let zero = storage
            .insert_plugin("zero", PluginType::Mcp, Transport::Local, 0, &env, &args)
            .await;
        assert!(matches!(zero, Err(StorageError::Sqlx(_))), "zero: {zero:?}");
    }
}
