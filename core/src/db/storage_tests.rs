use std::sync::LazyLock;

use paloma_provider_protocol::v1::{self, conversation_item::Item};
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

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}

static CODEX: LazyLock<ProviderBackendId> = LazyLock::new(|| ProviderBackendId {
    provider_id: "openai".into(),
    backend_id: "codex".into(),
});
static OPENAI: LazyLock<ProviderBackendId> = LazyLock::new(|| ProviderBackendId {
    provider_id: "openai".into(),
    backend_id: "openai".into(),
});
static ANTHROPIC: LazyLock<ProviderBackendId> = LazyLock::new(|| ProviderBackendId {
    provider_id: "anthropic".into(),
    backend_id: "anthropic".into(),
});

mod storage {
    use super::*;

    #[tokio::test]
    async fn new_creates_database() {
        let storage = fresh_storage().await;
        let providers = storage.connected_backends().await.expect("providers");
        assert!(providers.is_empty(), "expected empty, got {providers:?}");
    }

    #[tokio::test]
    async fn new_is_idempotent_across_reopens() {
        let uri = "file:paloma_reopen_idempotent?mode=memory&cache=shared";
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
            .insert_backend(
                &CODEX,
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
        let providers = second.connected_backends().await.expect("providers");
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
            .insert_backend(
                &CODEX,
                &AuthKind::ApiKey,
                "sk-abc",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .expect("insert");

        let row = sqlx::query(
            "SELECT provider_id, backend_id, auth_kind, secret FROM backend_credentials
             WHERE provider_id = ? AND backend_id = ?",
        )
        .bind("openai")
        .bind("codex")
        .fetch_one(storage.pool())
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("provider_id"), "openai");
        assert_eq!(row.get::<String, _>("backend_id"), "codex");
        assert_eq!(row.get::<String, _>("auth_kind"), "api_key");
        assert_eq!(row.get::<String, _>("secret"), "sk-abc");
    }

    #[tokio::test]
    async fn insert_provider_duplicate_returns_duplicate_error() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok-1", "gpt-5", "medium")
            .await
            .expect("first insert");

        let err = storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok-2", "gpt-5", "medium")
            .await
            .expect_err("second insert must fail");

        assert!(
            matches!(err, StorageError::DuplicateBackend(ref id) if id == &*CODEX),
            "expected DuplicateBackend(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn update_provider_changes_row() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "old-token", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .update_backend(&CODEX, &AuthKind::Oauth, "new-token")
            .await
            .expect("update");

        let row = sqlx::query(
            "SELECT secret FROM backend_credentials WHERE provider_id = ? AND backend_id = ?",
        )
        .bind("openai")
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
            .update_backend(&CODEX, &AuthKind::ApiKey, "x")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFoundBackend(ref id) if id == &*CODEX),
            "expected NotFoundBackend(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn update_preferences_changes_model_and_effort_only() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .update_preferences(&CODEX, "gpt-5-mini", "high")
            .await
            .expect("update prefs");

        let row = sqlx::query(
            "SELECT secret, model, effort FROM backend_credentials
             WHERE provider_id = ? AND backend_id = ?",
        )
        .bind("openai")
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
            .update_preferences(&CODEX, "gpt-5", "medium")
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFoundBackend(ref id) if id == &*CODEX),
            "expected NotFoundBackend(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_provider_removes_row() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(
                &CODEX,
                &AuthKind::ApiKey,
                "x",
                "claude-sonnet-4-5",
                "medium",
            )
            .await
            .unwrap();

        storage.delete_backend(&CODEX).await.expect("delete");

        let providers = storage.connected_backends().await.expect("providers");
        assert!(providers.is_empty(), "expected empty, got {providers:?}");
    }

    #[tokio::test]
    async fn deleting_provider_plugin_purges_its_credentials() {
        let storage = fresh_storage().await;
        storage
            .insert_plugin(
                "openai",
                PluginType::Provider,
                Transport::Local,
                300,
                &HashMap::new(),
                &PluginArgs::Local {
                    command: "paloma".into(),
                    args: vec![],
                },
                None,
            )
            .await
            .unwrap();
        for id in [&*CODEX, &*OPENAI, &*ANTHROPIC] {
            storage
                .insert_backend(id, &AuthKind::ApiKey, "x", "model", "medium")
                .await
                .unwrap();
        }

        storage
            .delete_plugin("openai")
            .await
            .expect("delete plugin");

        let remaining: Vec<ProviderBackendId> = storage
            .connected_backends()
            .await
            .expect("providers")
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(remaining, vec![ANTHROPIC.clone()]);
        // the trigger-driven purge must also reassign the preference away from
        // the deleted provider's backends
        assert_eq!(preferred_ids(&storage).await, vec![ANTHROPIC.clone()]);
        assert_eq!(
            storage.preferred_provider_backend_id().await.unwrap(),
            Some(ANTHROPIC.clone())
        );
    }

    #[tokio::test]
    async fn delete_provider_nonexistent_returns_not_found() {
        let storage = fresh_storage().await;
        let err = storage.delete_backend(&CODEX).await.expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFoundBackend(ref id) if id == &*CODEX),
            "expected NotFoundBackend(\"codex\"), got {err:?}",
        );
    }

    #[tokio::test]
    async fn delete_provider_nonexistent_keeps_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5", "medium")
            .await
            .unwrap();

        let err = storage
            .delete_backend(&ANTHROPIC)
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFoundBackend(ref id) if id == &*ANTHROPIC),
            "expected NotFoundBackend(\"anthropic\"), got {err:?}",
        );
        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);

        let providers = storage.connected_backends().await.expect("providers");
        assert_eq!(providers.len(), 2);
    }

    #[tokio::test]
    async fn connected_providers_returns_empty_when_no_rows() {
        let storage = fresh_storage().await;
        let rows = storage.connected_backends().await.expect("query");
        assert!(rows.is_empty(), "expected empty, got {rows:?}");
    }

    #[tokio::test]
    async fn connected_providers_returns_all_inserted_ids() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let ids: Vec<ProviderBackendId> = storage
            .connected_backends()
            .await
            .expect("query")
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, vec![CODEX.clone()]);
    }

    /// The providers currently flagged preferred — at most one under the
    /// insert / `set_preferred_provider_config` invariant.
    async fn preferred_ids(storage: &Storage) -> Vec<ProviderBackendId> {
        storage
            .connected_backends()
            .await
            .expect("connected providers")
            .into_iter()
            .filter(|p| p.preferred)
            .map(|p| p.id)
            .collect()
    }

    #[tokio::test]
    async fn insert_provider_first_is_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);
    }

    #[tokio::test]
    async fn insert_provider_later_is_not_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5", "medium")
            .await
            .unwrap();

        // First connect stays preferred; a later one does not steal it.
        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);
    }

    #[tokio::test]
    async fn set_preferred_provider_config_updates_config_and_preference() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5-mini", "low")
            .await
            .unwrap();

        storage
            .set_preferred_provider_backend_config(&OPENAI, "gpt-5.1", "high", true)
            .await
            .expect("set preferred");

        // Exactly one preferred, and it switched to the target.
        assert_eq!(preferred_ids(&storage).await, vec![OPENAI.clone()]);

        let config = storage
            .prefer_model_config(&OPENAI)
            .await
            .expect("openai config");
        assert_eq!(config.model, "gpt-5.1");
        assert_eq!(config.effort, "high");
    }

    #[tokio::test]
    async fn set_preferred_provider_config_without_default_updates_config_only() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5-mini", "low")
            .await
            .unwrap();

        storage
            .set_preferred_provider_backend_config(&OPENAI, "gpt-5.1", "high", false)
            .await
            .expect("update config");

        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);

        let config = storage
            .prefer_model_config(&OPENAI)
            .await
            .expect("openai config");
        assert_eq!(config.model, "gpt-5.1");
        assert_eq!(config.effort, "high");
    }

    #[tokio::test]
    async fn set_preferred_provider_config_nonexistent_keeps_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let err = storage
            .set_preferred_provider_backend_config(&OPENAI, "gpt-5-mini", "high", true)
            .await
            .expect_err("must fail");

        assert!(
            matches!(err, StorageError::NotFoundBackend(ref id) if id == &*OPENAI),
            "expected NotFoundBackend(\"openai\"), got {err:?}",
        );
        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);

        let config = storage
            .prefer_model_config(&CODEX)
            .await
            .expect("codex config");
        assert_eq!(config.model, "gpt-5");
        assert_eq!(config.effort, "medium");
    }

    #[tokio::test]
    async fn preferred_provider_returns_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5-mini", "high")
            .await
            .unwrap();

        storage
            .set_preferred_provider_backend_config(&OPENAI, "gpt-5-mini", "high", true)
            .await
            .expect("set preferred");

        let provider_id = storage
            .preferred_provider_backend_id()
            .await
            .expect("preferred provider");
        assert_eq!(provider_id, Some(OPENAI.clone()));
    }

    #[tokio::test]
    async fn delete_preferred_provider_promotes_survivor() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .delete_backend(&CODEX)
            .await
            .expect("delete preferred");

        // The lone survivor should inherit the preference.
        assert_eq!(preferred_ids(&storage).await, vec![OPENAI.clone()]);
    }

    #[tokio::test]
    async fn delete_preferred_keeps_one_preferred_among_survivors() {
        let storage = fresh_storage().await;
        for id in [CODEX.clone(), OPENAI.clone(), ANTHROPIC.clone()] {
            storage
                .insert_backend(&id, &AuthKind::ApiKey, "sk", "model", "medium")
                .await
                .unwrap();
        }

        storage
            .delete_backend(&CODEX)
            .await
            .expect("delete preferred");

        assert_eq!(preferred_ids(&storage).await, vec![OPENAI.clone()]);
    }

    #[tokio::test]
    async fn delete_non_preferred_provider_keeps_current_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5", "medium")
            .await
            .unwrap();

        storage
            .delete_backend(&OPENAI)
            .await
            .expect("delete non-preferred");

        assert_eq!(preferred_ids(&storage).await, vec![CODEX.clone()]);
    }

    #[tokio::test]
    async fn connect_into_empty_is_preferred() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();
        storage.delete_backend(&CODEX).await.expect("delete last");

        storage
            .insert_backend(&OPENAI, &AuthKind::ApiKey, "sk", "gpt-5", "medium")
            .await
            .unwrap();

        assert_eq!(preferred_ids(&storage).await, vec![OPENAI.clone()]);
    }
}

mod sessions {
    use super::*;

    #[tokio::test]
    async fn search_sessions_matches_prompts_and_messages() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let prompted = uuid("019e1234-5678-7000-8000-0000000000a1");
        let answered = uuid("019e1234-5678-7000-8000-0000000000a2");
        let unrelated = uuid("019e1234-5678-7000-8000-0000000000a3");
        for id in [prompted, answered, unrelated] {
            storage.create_new_session(id, "t").await.unwrap();
        }

        storage
            .insert_history(
                &prompted.to_string(),
                &CODEX,
                &ConversationItem {
                    item: Some(Item::UserPrompt(v1::UserPrompt {
                        prompt: "deploy the staging cluster".into(),
                    })),
                },
            )
            .await
            .unwrap();
        storage
            .insert_history(
                &answered.to_string(),
                &CODEX,
                &ConversationItem {
                    item: Some(Item::Message(v1::ConversationMessage {
                        message: vec![v1::MessageContentItem {
                            content: "Kubernetes upgrade notes".into(),
                            provider_meta: Default::default(),
                        }],
                        provider_meta: Default::default(),
                    })),
                },
            )
            .await
            .unwrap();
        storage
            .insert_history(
                &unrelated.to_string(),
                &CODEX,
                &ConversationItem {
                    item: Some(Item::UserPrompt(v1::UserPrompt {
                        prompt: "something else".into(),
                    })),
                },
            )
            .await
            .unwrap();

        let hits = storage.search_sessions("staging").await.expect("search");
        assert_eq!(hits, vec![prompted.to_string()]);

        // Case-insensitive, and assistant message content is searched too.
        let hits = storage.search_sessions("kubernetes").await.expect("search");
        assert_eq!(hits, vec![answered.to_string()]);

        // LIKE wildcards in the needle match literally, not everything.
        let hits = storage.search_sessions("100%").await.expect("search");
        assert!(hits.is_empty(), "expected no matches, got {hits:?}");
    }

    #[tokio::test]
    async fn create_session_persists_title_and_defaults() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = uuid("019e1234-5678-7000-8000-000000000001");
        storage
            .create_new_session(session_id, "my first chat")
            .await
            .expect("create session");

        let sessions = storage.all_sessions().await.expect("all sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id.to_string());
        assert_eq!(sessions[0].title, "my first chat");
        // `last_update` defaults to `unixepoch()` on insert.
        assert!(sessions[0].last_update > 0);
    }

    #[tokio::test]
    async fn all_sessions_orders_by_last_update_latest_first() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let oldest = uuid("019e1234-5678-7000-8000-00000000000a");
        let newest = uuid("019e1234-5678-7000-8000-00000000000b");
        let middle = uuid("019e1234-5678-7000-8000-00000000000c");
        for (id, title) in [(oldest, "oldest"), (newest, "newest"), (middle, "middle")] {
            storage.create_new_session(id, title).await.unwrap();
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

        let ordered: Vec<(String, i64)> = storage
            .all_sessions()
            .await
            .expect("all sessions")
            .into_iter()
            .map(|s| (s.title, s.last_update))
            .collect();
        assert_eq!(
            ordered,
            vec![
                ("newest".to_string(), 300),
                ("middle".to_string(), 200),
                ("oldest".to_string(), 100),
            ]
        );
    }

    #[tokio::test]
    async fn inserting_user_prompt_history_touches_session() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = uuid("019e1234-5678-7000-8000-000000000003");
        storage.create_new_session(session_id, "t").await.unwrap();

        // Backdate so the bump is observable regardless of clock resolution.
        sqlx::query("UPDATE sessions SET last_update = 1000 WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(storage.pool())
            .await
            .unwrap();

        storage
            .insert_history(
                &session_id.to_string(),
                &CODEX,
                &ConversationItem {
                    item: Some(Item::UserPrompt(v1::UserPrompt {
                        prompt: "hello".into(),
                    })),
                },
            )
            .await
            .expect("insert history");

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
    async fn inserting_non_prompt_history_preserves_session_timestamp() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = uuid("019e1234-5678-7000-8000-0000000000fe");
        storage.create_new_session(session_id, "t").await.unwrap();

        sqlx::query("UPDATE sessions SET last_update = 1000 WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(storage.pool())
            .await
            .unwrap();

        storage
            .insert_history(
                &session_id.to_string(),
                &CODEX,
                &ConversationItem {
                    item: Some(Item::Message(v1::ConversationMessage::default())),
                },
            )
            .await
            .expect("insert history");

        let row = sqlx::query("SELECT last_update FROM sessions WHERE session_id = ?")
            .bind(session_id.to_string())
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>("last_update"), 1000);
    }

    #[tokio::test]
    async fn delete_session_cascades_to_history() {
        let storage = fresh_storage().await;
        storage
            .insert_backend(&CODEX, &AuthKind::Oauth, "tok", "gpt-5", "medium")
            .await
            .unwrap();

        let session_id = uuid("019e1234-5678-7000-8000-000000000004");
        storage
            .create_new_session(session_id, "with history")
            .await
            .unwrap();

        let session_id = session_id.to_string();
        storage
            .insert_history(
                &session_id,
                &CODEX,
                &ConversationItem {
                    item: Some(Item::Message(v1::ConversationMessage::default())),
                },
            )
            .await
            .unwrap();
        storage
            .insert_history(
                &session_id,
                &CODEX,
                &ConversationItem {
                    item: Some(Item::Reasoning(v1::Reasoning::default())),
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
            .create_new_session(session_id, "round trip")
            .await
            .unwrap();

        let items = every_conversation_item();

        for item in &items {
            storage
                .insert_history(&session_id.to_string(), &CODEX, item)
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
                .all(|entry| entry.provider_backend_id.backend_id == CODEX.backend_id)
        );
    }

    #[tokio::test]
    async fn insert_history_writes_stored_json_shape() {
        let storage = fresh_storage().await;
        seed_provider(&storage).await;
        let id = uuid("019e1234-5678-7000-8000-0000000000ab");
        seed_session(&storage, id, &[user()]).await;

        // The payload column keeps the pre-protocol JSON shape the SQL queries
        // and existing rows rely on.
        let row = sqlx::query("SELECT payload_type, payload FROM history WHERE session_id = ?")
            .bind(id.to_string())
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("payload_type"), "user_prompt");
        assert_eq!(
            row.get::<String, _>("payload"),
            r#"{"kind":"user_prompt","prompt":"example prompt"}"#
        );
    }

    #[tokio::test]
    async fn history_preserves_provider_id_per_row() {
        let storage = fresh_storage().await;
        seed_provider_id(&storage, &CODEX).await;
        seed_provider_id(&storage, &ANTHROPIC).await;

        let session_id = uuid("019e1234-5678-7000-8000-000000000006");
        storage
            .create_new_session(session_id, "mixed providers")
            .await
            .unwrap();

        let user = user();
        let message = assistant_message();
        storage
            .insert_history(&session_id.to_string(), &CODEX, &user)
            .await
            .unwrap();
        storage
            .insert_history(&session_id.to_string(), &ANTHROPIC, &message)
            .await
            .unwrap();

        let history = storage.get_history(&session_id.to_string()).await.unwrap();

        assert_eq!(
            history
                .iter()
                .map(|entry| entry.provider_backend_id.clone())
                .collect::<Vec<_>>(),
            vec![CODEX.clone(), ANTHROPIC.clone()]
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
        seed_provider_id(&storage, &OPENAI).await;
        let session_id = uuid("019e1234-5678-7000-8000-000000000009");
        let pending_tool_call = function_call_with("pending_call");
        let finished_tool_call = function_call_with("finished_call");
        let tool_result = tool_result_with("finished_call");
        let hosted_tool = hosted_tool();
        let message = assistant_message();
        storage
            .create_new_session(session_id, "restore")
            .await
            .unwrap();
        for (provider_id, item) in [
            (CODEX.clone(), user()),
            (OPENAI.clone(), pending_tool_call.clone()),
            (CODEX.clone(), reasoning()),
            (OPENAI.clone(), finished_tool_call.clone()),
            (OPENAI.clone(), tool_result),
            (CODEX.clone(), hosted_tool.clone()),
            (OPENAI.clone(), message.clone()),
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
                .map(|entry| entry.provider_backend_id.clone())
                .collect::<Vec<_>>(),
            vec![
                CODEX.clone(),
                OPENAI.clone(),
                CODEX.clone(),
                OPENAI.clone(),
                CODEX.clone(),
                OPENAI.clone(),
            ]
        );
    }

    // ---- history ----

    async fn seed_provider(storage: &Storage) {
        seed_provider_id(storage, &CODEX).await;
    }

    async fn seed_provider_id(storage: &Storage, provider_id: &ProviderBackendId) {
        storage
            .insert_backend(provider_id, &AuthKind::Oauth, "tok", "gpt-5", "medium")
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
                "output": "/home/user/project"
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
        storage.create_new_session(id, "s").await.unwrap();
        for item in items {
            storage
                .insert_history(&id.to_string(), &CODEX, item)
                .await
                .unwrap();
        }
    }

    async fn history_len(storage: &Storage, id: Uuid) -> usize {
        storage.get_history(&id.to_string()).await.unwrap().len()
    }

    fn user() -> ConversationItem {
        ConversationItem {
            item: Some(Item::UserPrompt(v1::UserPrompt {
                prompt: "example prompt".to_string(),
            })),
        }
    }

    fn assistant_message() -> ConversationItem {
        ConversationItem {
            item: Some(Item::Message(v1::ConversationMessage::default())),
        }
    }

    fn reasoning() -> ConversationItem {
        ConversationItem {
            item: Some(Item::Reasoning(v1::Reasoning::default())),
        }
    }

    fn function_call() -> ConversationItem {
        function_call_with("c1")
    }

    fn function_call_with(call_id: &str) -> ConversationItem {
        ConversationItem {
            item: Some(Item::ToolCall(v1::ToolCall {
                call_id: call_id.to_string(),
                name: "example_tool".to_string(),
                arguments: "{}".to_string(),
                provider_meta: Default::default(),
            })),
        }
    }

    fn tool_result_with(call_id: &str) -> ConversationItem {
        ConversationItem {
            item: Some(Item::ToolResult(v1::ToolResult {
                call_id: call_id.to_string(),
                name: "example_tool".to_string(),
                output: "ok".to_string(),
            })),
        }
    }

    fn hosted_tool() -> ConversationItem {
        ConversationItem {
            item: Some(Item::HostedTool(v1::HostedTool {
                function_type: "web_search_call".to_string(),
                content: Some("searched docs".to_string()),
                provider_meta: Default::default(),
            })),
        }
    }

    async fn insert_invalid_history_payload(storage: &Storage, session_id: &str) {
        sqlx::query(
            "INSERT INTO history (session_id, provider_id, backend_id, payload_type, payload)
         VALUES (?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind("openai")
        .bind("codex")
        .bind("message")
        .bind(r#"{"kind":"unknown"}"#)
        .execute(storage.pool())
        .await
        .unwrap();
    }

    mod recover {
        use super::*;

        async fn snapshot(
            storage: &Storage,
            ids: &[Uuid],
        ) -> Vec<(bool, Vec<(ProviderBackendId, ConversationItem)>)> {
            let sessions = storage.all_sessions().await.unwrap();
            let mut out = Vec::new();
            for id in ids {
                let exists = sessions.iter().any(|s| s.session_id == id.to_string());
                let history = storage
                    .get_history(&id.to_string())
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|entry| (entry.provider_backend_id, entry.payload))
                    .collect();
                out.push((exists, history));
            }
            out
        }

        #[tokio::test]
        async fn given_an_open_tool_call_when_recovering_should_close_it_with_the_error_reason() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
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
            assert_eq!(history.len(), 6);
            let last = &history.last().unwrap().payload.item;
            let Some(Item::ToolResult(result)) = last else {
                panic!("expected a tool result, got {last:?}");
            };
            assert_eq!(result.call_id, "c1");
            assert_eq!(result.output, TURN_ERROR_REASON);
        }

        #[tokio::test]
        async fn given_a_completed_turn_when_recovering_should_keep_it() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), assistant_message()]).await;

            storage.recover_history().await.unwrap();

            assert_eq!(history_len(&storage, id).await, 2);
        }

        #[tokio::test]
        async fn given_only_a_prompt_and_reasoning_when_recovering_should_remove_the_session() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), reasoning()]).await;

            storage.recover_history().await.unwrap();

            assert_eq!(history_len(&storage, id).await, 0);
            let sessions = storage.all_sessions().await.unwrap();
            assert!(sessions.iter().all(|s| s.session_id != id.to_string()));
        }

        #[tokio::test]
        async fn given_a_trailing_prompt_when_recovering_should_drop_just_that_prompt() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), assistant_message(), user()]).await;

            storage.recover_history().await.unwrap();

            assert_eq!(history_len(&storage, id).await, 2);
        }

        #[tokio::test]
        async fn given_a_hosted_tool_turn_when_recovering_should_keep_it() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[user(), assistant_message(), user(), hosted_tool()],
            )
            .await;

            storage.recover_history().await.unwrap();

            assert_eq!(history_len(&storage, id).await, 4);
        }

        #[tokio::test]
        async fn given_trailing_reasoning_when_recovering_should_drop_it() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    function_call_with("c1"),
                    tool_result_with("c1"),
                    reasoning(),
                ],
            )
            .await;

            storage.recover_history().await.unwrap();

            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 3);
            assert!(matches!(
                history.last().unwrap().payload,
                ConversationItem {
                    item: Some(Item::ToolResult(_))
                }
            ));
        }

        #[tokio::test]
        async fn given_a_session_without_history_when_recovering_should_remove_it() {
            let storage = fresh_storage().await;
            let id = Uuid::now_v7();
            storage.create_new_session(id, "s").await.unwrap();

            storage.recover_history().await.unwrap();

            assert!(storage.all_sessions().await.unwrap().is_empty());
        }

        #[tokio::test]
        async fn given_many_broken_sessions_when_recovering_should_repair_each_in_one_pass() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;

            let finished_single = Uuid::now_v7();
            let finished_multi = Uuid::now_v7();
            let open_tool_call = Uuid::now_v7();
            let no_completed_turn = Uuid::now_v7();
            let dangling_prompt = Uuid::now_v7();

            seed_session(&storage, finished_single, &[user(), assistant_message()]).await;
            seed_session(
                &storage,
                finished_multi,
                &[user(), assistant_message(), user(), assistant_message()],
            )
            .await;
            seed_session(
                &storage,
                open_tool_call,
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
            assert_eq!(history_len(&storage, open_tool_call).await, 6);
            assert_eq!(history_len(&storage, no_completed_turn).await, 0);
            assert_eq!(history_len(&storage, dangling_prompt).await, 2);
        }

        #[tokio::test]
        async fn given_the_same_call_id_answered_elsewhere_when_recovering_should_still_close_the_open_one()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let answered = Uuid::now_v7();
            let open = Uuid::now_v7();
            seed_session(
                &storage,
                answered,
                &[user(), function_call_with("c1"), tool_result_with("c1")],
            )
            .await;
            seed_session(&storage, open, &[user(), function_call_with("c1")]).await;

            storage.recover_history().await.unwrap();

            assert_eq!(history_len(&storage, answered).await, 3);
            let history = storage.get_history(&open.to_string()).await.unwrap();
            assert_eq!(history.len(), 3);
            let Some(Item::ToolResult(result)) = &history[2].payload.item else {
                panic!("expected a tool result for the open call");
            };
            assert_eq!(result.output, TURN_ERROR_REASON);
        }

        #[tokio::test]
        async fn given_the_same_broken_sessions_when_recovering_or_cleaning_up_should_leave_identical_rows()
         {
            let shapes: Vec<Vec<ConversationItem>> = vec![
                vec![],
                vec![user()],
                vec![user(), reasoning()],
                vec![user(), assistant_message()],
                vec![user(), assistant_message(), user()],
                vec![user(), assistant_message(), user(), reasoning()],
                vec![user(), hosted_tool()],
                vec![user(), reasoning(), function_call_with("c1")],
                vec![user(), reasoning(), function_call_with("c1"), reasoning()],
                vec![
                    user(),
                    function_call_with("c1"),
                    tool_result_with("c1"),
                    function_call_with("c2"),
                ],
                vec![
                    user(),
                    function_call_with("c1"),
                    tool_result_with("c1"),
                    reasoning(),
                ],
                vec![assistant_message()],
            ];
            let ids: Vec<Uuid> = shapes.iter().map(|_| Uuid::now_v7()).collect();

            let cleaned = fresh_storage().await;
            let recovered = fresh_storage().await;
            for storage in [&cleaned, &recovered] {
                seed_provider(storage).await;
                for (id, items) in ids.iter().zip(&shapes) {
                    seed_session(storage, *id, items).await;
                }
            }

            for id in &ids {
                cleaned
                    .cleanup_session_history(&id.to_string(), TURN_ERROR_REASON)
                    .await
                    .unwrap();
            }
            recovered.recover_history().await.unwrap();

            let cleaned = snapshot(&cleaned, &ids).await;
            assert_eq!(cleaned, snapshot(&recovered, &ids).await);

            let closed_calls = cleaned
                .iter()
                .flat_map(|(_, history)| history)
                .filter(|(_, item)| {
                    matches!(&item.item, Some(Item::ToolResult(r)) if r.output == TURN_ERROR_REASON)
                })
                .count();
            assert_eq!(closed_calls, 3);
        }
    }

    mod cleanup {
        use super::*;

        const REASON: &str = "cancelled by user";

        async fn cleanup(storage: &Storage, id: Uuid) -> bool {
            storage
                .cleanup_session_history(&id.to_string(), REASON)
                .await
                .unwrap()
        }

        async fn session_exists(storage: &Storage, id: Uuid) -> bool {
            storage
                .all_sessions()
                .await
                .unwrap()
                .iter()
                .any(|s| s.session_id == id.to_string())
        }

        fn tool_results(history: &[HistoryEntry]) -> Vec<&v1::ToolResult> {
            history
                .iter()
                .filter_map(|entry| match &entry.payload.item {
                    Some(Item::ToolResult(result)) => Some(result),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn given_a_prompt_only_turn_when_cleaning_up_should_remove_turn_and_session() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user()]).await;

            assert!(cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 0);
            assert!(!session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_a_prompt_with_only_reasoning_when_cleaning_up_should_remove_turn_and_session()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), reasoning()]).await;

            assert!(cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 0);
            assert!(!session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_a_prompt_only_second_turn_when_cleaning_up_should_drop_just_that_prompt() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), assistant_message(), user()]).await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 2);
            assert!(matches!(
                history.last().unwrap().payload,
                ConversationItem {
                    item: Some(Item::Message(_))
                }
            ));
            assert!(session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_an_unanswered_tool_call_when_cleaning_up_should_add_a_result_with_the_reason()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[user(), reasoning(), function_call_with("c1")],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 4);
            let results = tool_results(&history);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].call_id, "c1");
            assert_eq!(results[0].name, "example_tool");
            assert_eq!(results[0].output, REASON);
            assert_eq!(history[3].provider_backend_id, *CODEX);
        }

        #[tokio::test]
        async fn given_answered_tool_calls_when_cleaning_up_should_leave_them_alone() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    function_call_with("c1"),
                    tool_result_with("c1"),
                    function_call_with("c2"),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 5);
            let results = tool_results(&history);
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].call_id, "c1");
            assert_eq!(results[0].output, "ok");
            assert_eq!(results[1].call_id, "c2");
            assert_eq!(results[1].output, REASON);
        }

        #[tokio::test]
        async fn given_a_completed_turn_when_cleaning_up_should_change_nothing() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), assistant_message()]).await;

            assert!(!cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 2);
            assert!(session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_an_open_call_in_an_earlier_turn_when_cleaning_up_should_only_touch_the_latest_turn()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    function_call_with("c1"),
                    assistant_message(),
                    user(),
                    function_call_with("c2"),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 6);
            let results = tool_results(&history);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].call_id, "c2");
        }

        #[tokio::test]
        async fn given_another_broken_session_when_cleaning_up_should_only_touch_the_target_session()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let target = Uuid::now_v7();
            let other = Uuid::now_v7();
            seed_session(&storage, target, &[user(), reasoning(), function_call()]).await;
            seed_session(&storage, other, &[user(), reasoning(), function_call()]).await;

            assert!(!cleanup(&storage, target).await);
            assert_eq!(history_len(&storage, target).await, 4);
            assert_eq!(history_len(&storage, other).await, 3);
            assert!(session_exists(&storage, other).await);
        }

        #[tokio::test]
        async fn given_a_session_without_a_prompt_when_cleaning_up_should_change_nothing() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[assistant_message()]).await;

            assert!(!cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 1);
            assert!(session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_a_closed_call_when_restoring_history_should_mark_it_finished() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), function_call_with("c1")]).await;

            assert!(!cleanup(&storage, id).await);
            let entries = storage.restore_history(&id.to_string()).await.unwrap();
            assert_eq!(entries.len(), 2);
            assert!(matches!(
                entries[1].payload,
                ConversationItem {
                    item: Some(Item::ToolCall(_))
                }
            ));
            assert!(entries[1].finished);
        }

        #[tokio::test]
        async fn given_a_call_from_another_provider_when_cleaning_up_should_copy_its_provider_to_the_result()
         {
            let storage = fresh_storage().await;
            seed_provider_id(&storage, &CODEX).await;
            seed_provider_id(&storage, &ANTHROPIC).await;
            let id = Uuid::now_v7();
            storage.create_new_session(id, "s").await.unwrap();
            storage
                .insert_history(&id.to_string(), &CODEX, &user())
                .await
                .unwrap();
            storage
                .insert_history(&id.to_string(), &ANTHROPIC, &function_call_with("c1"))
                .await
                .unwrap();

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 3);
            assert_eq!(history[2].provider_backend_id, *ANTHROPIC);
            assert_eq!(tool_results(&history)[0].call_id, "c1");
        }

        #[tokio::test]
        async fn given_parallel_calls_when_cleaning_up_should_only_close_the_unanswered_ones() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    function_call_with("c1"),
                    function_call_with("c2"),
                    function_call_with("c3"),
                    tool_result_with("c1"),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 7);
            let results = tool_results(&history);
            assert_eq!(results.len(), 3);
            assert_eq!(results[0].call_id, "c1");
            assert_eq!(results[0].output, "ok");
            let mut closed: Vec<&str> = results[1..]
                .iter()
                .map(|result| result.call_id.as_str())
                .collect();
            closed.sort_unstable();
            assert_eq!(closed, ["c2", "c3"]);
            assert!(results[1..].iter().all(|result| result.output == REASON));
        }

        #[tokio::test]
        async fn given_an_already_cleaned_turn_when_cleaning_up_again_should_add_nothing() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), function_call_with("c1")]).await;

            assert!(!cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 3);

            assert!(!cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 3);
        }

        #[tokio::test]
        async fn given_a_hosted_tool_turn_when_cleaning_up_should_keep_it() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(&storage, id, &[user(), hosted_tool()]).await;

            assert!(!cleanup(&storage, id).await);
            assert_eq!(history_len(&storage, id).await, 2);
            assert!(session_exists(&storage, id).await);
        }

        #[tokio::test]
        async fn given_trailing_reasoning_in_a_kept_turn_when_cleaning_up_should_drop_it() {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    reasoning(),
                    function_call_with("c1"),
                    tool_result_with("c1"),
                    reasoning(),
                    reasoning(),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 4);
            assert!(matches!(
                history[1].payload,
                ConversationItem {
                    item: Some(Item::Reasoning(_))
                }
            ));
            assert!(matches!(
                history[3].payload,
                ConversationItem {
                    item: Some(Item::ToolResult(_))
                }
            ));
            assert_eq!(tool_results(&history).len(), 1);
        }

        #[tokio::test]
        async fn given_trailing_reasoning_in_an_earlier_turn_when_cleaning_up_should_leave_it_alone()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    assistant_message(),
                    reasoning(),
                    user(),
                    function_call_with("c1"),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 6);
            assert!(matches!(
                history[2].payload,
                ConversationItem {
                    item: Some(Item::Reasoning(_))
                }
            ));
        }

        #[tokio::test]
        async fn given_trailing_reasoning_after_an_open_call_when_cleaning_up_should_drop_it_and_close_the_call()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[user(), reasoning(), function_call_with("c1"), reasoning()],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 4);
            assert!(matches!(
                history[1].payload,
                ConversationItem {
                    item: Some(Item::Reasoning(_))
                }
            ));
            assert!(matches!(
                history[2].payload,
                ConversationItem {
                    item: Some(Item::ToolCall(_))
                }
            ));
            let results = tool_results(&history);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].call_id, "c1");
            assert_eq!(results[0].output, REASON);
        }

        #[tokio::test]
        async fn given_a_second_turn_with_only_reasoning_when_cleaning_up_should_drop_it_and_keep_the_session()
         {
            let storage = fresh_storage().await;
            seed_provider(&storage).await;
            let id = Uuid::now_v7();
            seed_session(
                &storage,
                id,
                &[
                    user(),
                    reasoning(),
                    assistant_message(),
                    user(),
                    reasoning(),
                ],
            )
            .await;

            assert!(!cleanup(&storage, id).await);
            let history = storage.get_history(&id.to_string()).await.unwrap();
            assert_eq!(history.len(), 3);
            assert!(matches!(
                history[2].payload,
                ConversationItem {
                    item: Some(Item::Message(_))
                }
            ));
            assert!(session_exists(&storage, id).await);
        }
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

        assert!(matches!(err, StorageError::NotFoundPermission(ref id) if id == "cargo build"));
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
            .insert_plugin(
                "ok",
                PluginType::Mcp,
                Transport::Local,
                300,
                &env,
                &args,
                None,
            )
            .await
            .expect("positive timeout should insert");
        let zero = storage
            .insert_plugin(
                "zero",
                PluginType::Mcp,
                Transport::Local,
                0,
                &env,
                &args,
                None,
            )
            .await;
        assert!(matches!(zero, Err(StorageError::Sqlx(_))), "zero: {zero:?}");
    }

    #[tokio::test]
    async fn insert_plugin_accepts_extension_type() {
        let storage = fresh_storage().await;
        let env = HashMap::new();
        let args = PluginArgs::Local {
            command: "my-extension".to_string(),
            args: vec![],
        };
        storage
            .insert_plugin(
                "MyExtension",
                PluginType::Extension,
                Transport::Local,
                300,
                &env,
                &args,
                None,
            )
            .await
            .expect("extension plugin should insert");

        let plugins = storage
            .plugins_by_type(PluginType::Extension)
            .await
            .expect("query extensions");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "MyExtension");
    }
}

mod capabilities {
    use super::*;

    async fn plugin(storage: &Storage, name: &str, plugin_type: PluginType) {
        let args = PluginArgs::Local {
            command: "bin".to_string(),
            args: vec![],
        };
        storage
            .insert_plugin(
                name,
                plugin_type,
                Transport::Local,
                300,
                &HashMap::new(),
                &args,
                None,
            )
            .await
            .expect("insert plugin");
    }

    fn row(
        plugin: &str,
        capability: &str,
        facet: CapabilityFacet,
    ) -> (String, String, CapabilityFacet) {
        (plugin.to_string(), capability.to_string(), facet)
    }

    #[tokio::test]
    async fn disabled_capabilities_filter_on_requested_facets() {
        let storage = fresh_storage().await;

        // "Internal" is a built-in: no plugins row exists, toggling must
        // still work.
        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Tool, true)
            .await
            .expect("disable tool facet");
        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Search, true)
            .await
            .expect("disable search facet");
        storage
            .toggle_capability("fs", "read_file", CapabilityFacet::Mcp, true)
            .await
            .expect("disable mcp tool");

        let for_tools = storage
            .disabled_capabilities(&[CapabilityFacet::Tool, CapabilityFacet::Mcp])
            .await
            .expect("list tool and mcp");
        assert_eq!(for_tools.len(), 2);
        assert!(for_tools.contains(&row("Internal", "Files", CapabilityFacet::Tool)));
        assert!(for_tools.contains(&row("fs", "read_file", CapabilityFacet::Mcp)));

        let for_search = storage
            .disabled_capabilities(&[CapabilityFacet::Search])
            .await
            .expect("list search");
        assert_eq!(
            for_search.into_iter().collect::<Vec<_>>(),
            [row("Internal", "Files", CapabilityFacet::Search)]
        );
    }

    #[tokio::test]
    async fn toggle_capability_tracks_each_facet_independently() {
        let storage = fresh_storage().await;
        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Tool, true)
            .await
            .expect("disable tool facet");
        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Search, true)
            .await
            .expect("disable search facet");

        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Tool, false)
            .await
            .expect("re-enable tool facet");

        let tools = storage
            .disabled_capabilities(&[CapabilityFacet::Tool])
            .await
            .expect("list tools");
        assert!(tools.is_empty());
        let searches = storage
            .disabled_capabilities(&[CapabilityFacet::Search])
            .await
            .expect("list searches");
        assert!(searches.contains(&row("Internal", "Files", CapabilityFacet::Search)));
    }

    #[tokio::test]
    async fn toggle_capability_is_idempotent() {
        let storage = fresh_storage().await;

        for _ in 0..2 {
            storage
                .toggle_capability("fs", "read_file", CapabilityFacet::Mcp, true)
                .await
                .expect("disable");
        }
        let disabled = storage
            .disabled_capabilities(&[CapabilityFacet::Mcp])
            .await
            .expect("list");
        assert_eq!(disabled.len(), 1);

        for _ in 0..2 {
            storage
                .toggle_capability("fs", "read_file", CapabilityFacet::Mcp, false)
                .await
                .expect("enable");
        }
        let disabled = storage
            .disabled_capabilities(&[CapabilityFacet::Mcp])
            .await
            .expect("list");
        assert!(disabled.is_empty());
    }

    #[tokio::test]
    async fn delete_plugin_removes_its_capability_flags() {
        let storage = fresh_storage().await;
        plugin(&storage, "fs", PluginType::Mcp).await;
        storage
            .toggle_capability("fs", "read_file", CapabilityFacet::Mcp, true)
            .await
            .expect("disable mcp tool");
        storage
            .toggle_capability("Internal", "Files", CapabilityFacet::Tool, true)
            .await
            .expect("disable extension tool");

        storage.delete_plugin("fs").await.expect("delete plugin");

        let disabled = storage
            .disabled_capabilities(&[CapabilityFacet::Tool, CapabilityFacet::Mcp])
            .await
            .expect("list");
        assert_eq!(
            disabled.into_iter().collect::<Vec<_>>(),
            [row("Internal", "Files", CapabilityFacet::Tool)]
        );
    }
}
