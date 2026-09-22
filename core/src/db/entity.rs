use paloma_provider_protocol::v1::ConversationItem;
use sqlx::FromRow;

use crate::entity::ProviderBackendId;

#[derive(Clone, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum AuthKind {
    ApiKey,
    Oauth,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ConnectedBackend {
    #[sqlx(flatten)]
    pub id: ProviderBackendId,
    pub auth_kind: AuthKind,
    pub secret: String,
    pub model: String,
    pub effort: String,
    pub preferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Session {
    pub session_id: String,
    pub title: String,
    pub last_update: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct PreferModelConfig {
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Permission {
    pub prefix: String,
    pub with_glob: bool,
    pub updated_at: i64,
}

#[derive(Clone, Debug, FromRow)]
pub struct HistoryEntry {
    #[sqlx(flatten)]
    pub provider_backend_id: ProviderBackendId,
    #[sqlx(json)]
    pub payload: ConversationItem,
}

#[derive(Debug, FromRow)]
pub struct RestoreEntry {
    #[sqlx(flatten)]
    pub provider_backend_id: ProviderBackendId,
    #[sqlx(json)]
    pub payload: ConversationItem,
    pub finished: bool,
}

#[derive(Debug, FromRow)]
pub struct HistoryRow {
    pub id: i64,
    #[sqlx(flatten)]
    pub provider_backend_id: ProviderBackendId,
    #[sqlx(json)]
    pub payload: ConversationItem,
    #[sqlx(default)]
    pub finished: bool,
    pub ordinal: Option<i64>,
    pub kind: Option<String>,
    pub media_type: Option<String>,
    pub data: Option<Vec<u8>>,
}
