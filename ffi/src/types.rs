//! FFI boundary types for `scry-core`.
//!
//! Types that cross the boundary unchanged are declared as UniFFI *remote*
//! types: each declaration restates the core type's shape and UniFFI
//! generates the plumbing for the core type itself — no duplicate type, no
//! conversions, and a shape drift breaks the build. Types the FFI
//! deliberately reshapes (renamed fields, flattened payloads, JSON strings)
//! keep hand-written mirrors with conversions at the edge.

use std::collections::HashMap;

pub use scry_core::{
    Action, ActionOutcome, Connector, ConnectorConnection, HealthLevel, HealthStatus, ImageFormat,
    McpServer, Model, Permission, PermissionState, Plugin, PluginArgs, PluginType, ProviderId,
    ProviderStatus, SessionListItem, Transport, UserDecision,
};
use uuid::Uuid;

use crate::error::ScryError;

// Swift sees session ids as plain strings.
uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Ok(Uuid::parse_str(&value)?),
});

#[uniffi::remote(Enum)]
pub enum ProviderId {
    Codex,
    ClaudeCode,
    OpenAI,
    Anthropic,
}

#[uniffi::remote(Enum)]
pub enum HealthStatus {
    Starting,
    Running,
    Unhealthy,
}

#[uniffi::remote(Enum)]
pub enum HealthLevel {
    Inactive,
    Healthy,
    Degraded,
    Down,
}

#[uniffi::remote(Enum)]
pub enum PluginType {
    Native,
    Mcp,
}

#[uniffi::remote(Enum)]
pub enum Transport {
    Local,
    Http,
}

#[uniffi::remote(Enum)]
pub enum PluginArgs {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

#[uniffi::remote(Record)]
pub struct Plugin {
    pub name: String,
    pub transport: Transport,
    pub timeout: u32,
    pub disabled: bool,
    pub env: HashMap<String, String>,
    pub args: PluginArgs,
}

#[uniffi::remote(Record)]
pub struct McpServer {
    pub config: Plugin,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
}

#[uniffi::remote(Record)]
pub struct Permission {
    pub prefix: String,
    pub with_glob: bool,
    pub updated_at: i64,
}

#[uniffi::remote(Enum)]
pub enum PermissionState {
    Allow,
    Deny,
    Error,
}

#[uniffi::remote(Enum)]
pub enum UserDecision {
    AllowOnce {
        call_id: String,
    },
    Allow {
        call_id: String,
        command: String,
        glob: bool,
    },
    AllowSession {
        session_id: Uuid,
        call_id: String,
    },
    IgnorePermission {
        session_id: Uuid,
        call_id: String,
    },
    Deny {
        call_id: String,
    },
}

#[uniffi::remote(Record)]
pub struct SessionListItem {
    pub session_id: Uuid,
    pub title: String,
    pub last_update: i64,
}

#[uniffi::remote(Record)]
pub struct Action {
    pub label: String,
    pub params: Vec<String>,
    pub primary: bool,
}

#[uniffi::remote(Enum)]
pub enum ActionOutcome {
    Hide,
    Stay,
    Replace { input: String },
}

#[uniffi::remote(Enum)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
    Webp,
    Gif,
}

#[uniffi::remote(Record)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct ProviderStatus {
    pub models: Vec<Model>,
    pub status: HealthStatus,
    pub error: Option<String>,
}

#[uniffi::remote(Record)]
pub struct ConnectorConnection {
    pub preferred: bool,
    pub prefer_model: String,
    pub prefer_effort: String,
    pub status: ProviderStatus,
}

#[uniffi::remote(Record)]
pub struct Connector {
    pub id: ProviderId,
    pub connection: Option<ConnectorConnection>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Connection {
    /// User opens a URL and types a code into the page. The whole value must
    /// be passed back to `finalize_connection` unchanged.
    DeviceCode {
        verification_uri: String,
        user_code: String,
        transaction_payload_json: String,
    },
    /// User opens the URL; the browser redirects back to a local callback.
    /// Pass the same value back to `finalize_connection` to complete it.
    BrowserRedirect { authorization_url: String },
    /// User pastes an API key; construct this with the entered key and pass
    /// it to `finalize_connection`.
    ManualInput {
        api_key: String,
        instructions_url: Option<String>,
    },
}

impl From<scry_core::Connection> for Connection {
    fn from(value: scry_core::Connection) -> Self {
        match value {
            scry_core::Connection::DeviceCode {
                verification_uri,
                user_code,
                transaction_payload,
            } => Self::DeviceCode {
                verification_uri: verification_uri.to_owned(),
                user_code,
                transaction_payload_json: transaction_payload.to_string(),
            },
            scry_core::Connection::BrowserRedirect { authorization_url } => {
                Self::BrowserRedirect { authorization_url }
            },
            scry_core::Connection::ManualInput {
                api_key,
                instructions_url,
            } => Self::ManualInput {
                api_key,
                instructions_url,
            },
        }
    }
}

impl TryFrom<Connection> for scry_core::Connection {
    type Error = ScryError;

    fn try_from(value: Connection) -> Result<Self, Self::Error> {
        match value {
            Connection::DeviceCode {
                verification_uri,
                user_code,
                transaction_payload_json,
            } => Ok(Self::DeviceCode {
                // Core models this as &'static str because providers hard-code
                // it; after the FFI round-trip it is runtime data, so leak it.
                // Bounded: a few bytes once per device-code finalize.
                verification_uri: Box::leak(verification_uri.into_boxed_str()),
                user_code,
                transaction_payload: serde_json::from_str(&transaction_payload_json).map_err(
                    |e| ScryError::new(format!("invalid device-code transaction payload: {e}")),
                )?,
            }),
            Connection::BrowserRedirect { authorization_url } => {
                Ok(Self::BrowserRedirect { authorization_url })
            },
            Connection::ManualInput {
                api_key,
                instructions_url,
            } => Ok(Self::ManualInput {
                api_key,
                instructions_url,
            }),
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum IconRef {
    Name { name: String },
    Path { path: String },
    Embedded { format: ImageFormat, data: Vec<u8> },
}

impl From<scry_core::IconRef> for IconRef {
    fn from(value: scry_core::IconRef) -> Self {
        match value {
            scry_core::IconRef::Name(name) => Self::Name { name },
            scry_core::IconRef::Path(path) => Self::Path { path },
            scry_core::IconRef::Embedded { format, data } => Self::Embedded { format, data },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Item {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<IconRef>,
    pub actions: Vec<Action>,
}

impl From<scry_core::Item> for Item {
    fn from(value: scry_core::Item) -> Self {
        Self {
            title: value.title,
            subtitle: value.subtitle,
            icon: value.icon.map(Into::into),
            actions: value.actions,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct QueryResponse {
    /// handler unique name
    pub id: String,
    /// Display section name
    pub name: String,
    /// handler results
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum SearchRenderEvent {
    Append { response: QueryResponse },
}

impl From<scry_core::SearchRenderEvent> for SearchRenderEvent {
    fn from(value: scry_core::SearchRenderEvent) -> Self {
        match value {
            scry_core::SearchRenderEvent::Append { response } => Self::Append {
                response: QueryResponse {
                    id: response.id.to_owned(),
                    name: response.name,
                    items: response.items.into_iter().map(Into::into).collect(),
                },
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ChatRenderEvent {
    UserPrompt {
        text: String,
    },
    TextDelta {
        provider_id: ProviderId,
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCall {
        name: String,
        arguments: String,
        description: Option<String>,
        decisions: Vec<UserDecision>,
    },
}

impl From<scry_core::ChatRenderEvent> for ChatRenderEvent {
    fn from(value: scry_core::ChatRenderEvent) -> Self {
        match value {
            scry_core::ChatRenderEvent::UserPrompt { text } => Self::UserPrompt { text },
            scry_core::ChatRenderEvent::TextDelta { provider_id, text } => {
                Self::TextDelta { provider_id, text }
            },
            scry_core::ChatRenderEvent::ReasoningDelta { text } => Self::ReasoningDelta { text },
            scry_core::ChatRenderEvent::ToolCall {
                name,
                arguments,
                description,
                decisions,
            } => Self::ToolCall {
                name,
                arguments,
                description,
                decisions,
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum RenderEvent {
    Search { event: SearchRenderEvent },
    Chat { event: ChatRenderEvent },
    Cancel,
    Done,
    Error { message: String },
}

impl From<scry_core::RenderEvent> for RenderEvent {
    fn from(value: scry_core::RenderEvent) -> Self {
        match value {
            scry_core::RenderEvent::Search(event) => Self::Search {
                event: event.into(),
            },
            scry_core::RenderEvent::Chat(event) => Self::Chat {
                event: event.into(),
            },
            scry_core::RenderEvent::Cancel => Self::Cancel,
            scry_core::RenderEvent::Done => Self::Done,
            scry_core::RenderEvent::Error { message } => Self::Error { message },
        }
    }
}
