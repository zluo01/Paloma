use serde_json::Value;
use sqlx::FromRow;

use crate::entity::ProviderId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum AuthKind {
    ApiKey,
    Oauth,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ConnectedProvider {
    pub provider_id: ProviderId,
    pub auth_kind: AuthKind,
    pub secret: String,
    pub model: String,
    pub effort: String,
    pub preferred: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Permission {
    pub prefix: String,
    pub with_glob: bool,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum EntryType {
    ResponseItem,
    EventMsg,
}

#[derive(Debug, FromRow)]
pub struct FileEntry {
    pub payload_type: EntryType,
    pub payload: Value,
}

#[derive(Debug, FromRow)]
pub struct RestoreEntry {
    pub payload_type: EntryType,
    pub payload: Value,
    pub finished: bool,
}
