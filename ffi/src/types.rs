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
    Action, ActionOutcome, Connector, ConnectorConnection, HealthLevel, HealthStatus, IconRef,
    Item, McpServer, Model, Permission, PermissionState, Plugin, PluginArgs, PluginType,
    ProviderAuthMethod, ProviderBackendId, ProviderInfo, ProviderStatus, SessionListItem,
    Transport, UserDecision,
};
use uuid::Uuid;

use crate::error::ScryError;

// Swift sees session ids as plain strings.
uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Ok(Uuid::parse_str(&value)?),
});

#[uniffi::remote(Record)]
pub struct ProviderBackendId {
    pub provider_id: String,
    pub backend_id: String,
}

#[uniffi::remote(Enum)]
pub enum ProviderAuthMethod {
    Unknown,
    ApiKey,
    DeviceCode,
    BrowserOauth,
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
    Provider,
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
pub struct ProviderInfo {
    pub name: String,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
    pub config: Option<Plugin>,
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
pub enum IconRef {
    Name(String),
    Path(String),
    Embedded(Vec<u8>),
}

#[uniffi::remote(Record)]
pub struct Item {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<IconRef>,
    pub actions: Vec<Action>,
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
    pub id: ProviderBackendId,
    pub description: String,
    pub icon: Option<IconRef>,
    pub connection: Option<ConnectorConnection>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Connection {
    DeviceCode {
        verification_url: String,
        user_code: String,
        transaction_payload: String,
    },
    BrowserRedirect {
        authorization_url: String,
    },
    ManualInput {
        instructions_url: Option<String>,
    },
}

impl TryFrom<scry_core::ConnectionPayload> for Connection {
    type Error = ScryError;

    fn try_from(value: scry_core::ConnectionPayload) -> Result<Self, Self::Error> {
        use scry_core::connection_payload::Payload;
        match value.payload {
            Some(Payload::DeviceCode(device_code)) => Ok(Self::DeviceCode {
                verification_url: device_code.verification_url,
                user_code: device_code.user_code,
                transaction_payload: device_code.transaction_payload,
            }),
            Some(Payload::BrowserRedirect(redirect)) => Ok(Self::BrowserRedirect {
                authorization_url: redirect.authorization_url,
            }),
            Some(Payload::ManualInput(manual_input)) => Ok(Self::ManualInput {
                instructions_url: manual_input.instructions_url,
            }),
            // should not happen, this indicates a provider plugin bug.
            None => Err(ScryError::new(
                "provider returned an empty connection payload",
            )),
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
                    items: response.items,
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
        provider_backend_id: ProviderBackendId,
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
            scry_core::ChatRenderEvent::TextDelta {
                provider_backend_id,
                text,
            } => Self::TextDelta {
                provider_backend_id,
                text,
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
