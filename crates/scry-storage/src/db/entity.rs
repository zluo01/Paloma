use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum EntryType {
    ResponseItem,
    EventMsg,
}

impl EntryType {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryType::ResponseItem => "response_item",
            EntryType::EventMsg => "event_msg",
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum PluginType {
    Native,
    Mcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum Transport {
    Local,
    Http,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginArgs {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct Plugin {
    pub name: String,
    pub transport: Transport,
    pub timeout: i64,
    pub disabled: bool,
    #[sqlx(json)]
    pub env: HashMap<String, String>,
    #[sqlx(json)]
    pub args: PluginArgs,
}
