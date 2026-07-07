//! FFI mirrors of the `scry-core` public types.
//!
//! UniFFI derives can only live on types defined in this crate, so every type
//! crossing the boundary is mirrored here 1:1 and converted at the edge. Types
//! that only flow core -> Swift get `From<core>`; types Swift also constructs
//! (plugins, actions, decisions, connections) get the reverse conversion too.

use std::collections::HashMap;

use uuid::Uuid;

use crate::error::ScryError;

// Swift sees session ids as plain strings.
uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |value| Ok(Uuid::parse_str(&value)?),
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderId {
    Codex,
    ClaudeCode,
    OpenAI,
    Anthropic,
}

impl From<scry_core::ProviderId> for ProviderId {
    fn from(value: scry_core::ProviderId) -> Self {
        match value {
            scry_core::ProviderId::Codex => Self::Codex,
            scry_core::ProviderId::ClaudeCode => Self::ClaudeCode,
            scry_core::ProviderId::OpenAI => Self::OpenAI,
            scry_core::ProviderId::Anthropic => Self::Anthropic,
        }
    }
}

impl From<ProviderId> for scry_core::ProviderId {
    fn from(value: ProviderId) -> Self {
        match value {
            ProviderId::Codex => Self::Codex,
            ProviderId::ClaudeCode => Self::ClaudeCode,
            ProviderId::OpenAI => Self::OpenAI,
            ProviderId::Anthropic => Self::Anthropic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HealthStatus {
    Starting,
    Running,
    Unhealthy,
}

impl From<scry_core::HealthStatus> for HealthStatus {
    fn from(value: scry_core::HealthStatus) -> Self {
        match value {
            scry_core::HealthStatus::Starting => Self::Starting,
            scry_core::HealthStatus::Running => Self::Running,
            scry_core::HealthStatus::Unhealthy => Self::Unhealthy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HealthLevel {
    Inactive,
    Healthy,
    Degraded,
    Down,
}

impl From<scry_core::HealthLevel> for HealthLevel {
    fn from(value: scry_core::HealthLevel) -> Self {
        match value {
            scry_core::HealthLevel::Inactive => Self::Inactive,
            scry_core::HealthLevel::Healthy => Self::Healthy,
            scry_core::HealthLevel::Degraded => Self::Degraded,
            scry_core::HealthLevel::Down => Self::Down,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginType {
    Native,
    Mcp,
}

impl From<PluginType> for scry_core::PluginType {
    fn from(value: PluginType) -> Self {
        match value {
            PluginType::Native => Self::Native,
            PluginType::Mcp => Self::Mcp,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Transport {
    Local,
    Http,
}

impl From<scry_core::Transport> for Transport {
    fn from(value: scry_core::Transport) -> Self {
        match value {
            scry_core::Transport::Local => Self::Local,
            scry_core::Transport::Http => Self::Http,
        }
    }
}

impl From<Transport> for scry_core::Transport {
    fn from(value: Transport) -> Self {
        match value {
            Transport::Local => Self::Local,
            Transport::Http => Self::Http,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum PluginArgs {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

impl From<scry_core::PluginArgs> for PluginArgs {
    fn from(value: scry_core::PluginArgs) -> Self {
        match value {
            scry_core::PluginArgs::Local { command, args } => Self::Local { command, args },
            scry_core::PluginArgs::Remote { url, requires_auth } => {
                Self::Remote { url, requires_auth }
            },
        }
    }
}

impl From<PluginArgs> for scry_core::PluginArgs {
    fn from(value: PluginArgs) -> Self {
        match value {
            PluginArgs::Local { command, args } => Self::Local { command, args },
            PluginArgs::Remote { url, requires_auth } => Self::Remote { url, requires_auth },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Plugin {
    pub name: String,
    pub transport: Transport,
    pub timeout: u32,
    pub disabled: bool,
    pub env: HashMap<String, String>,
    pub args: PluginArgs,
}

impl From<scry_core::Plugin> for Plugin {
    fn from(value: scry_core::Plugin) -> Self {
        Self {
            name: value.name,
            transport: value.transport.into(),
            timeout: value.timeout,
            disabled: value.disabled,
            env: value.env,
            args: value.args.into(),
        }
    }
}

impl From<Plugin> for scry_core::Plugin {
    fn from(value: Plugin) -> Self {
        Self {
            name: value.name,
            transport: value.transport.into(),
            timeout: value.timeout,
            disabled: value.disabled,
            env: value.env,
            args: value.args.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct McpServer {
    pub config: Plugin,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
}

impl From<scry_core::McpServer> for McpServer {
    fn from(value: scry_core::McpServer) -> Self {
        Self {
            config: value.config.into(),
            description: value.description,
            status: value.status.into(),
            error: value.error,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Permission {
    pub prefix: String,
    pub with_glob: bool,
    pub updated_at: i64,
}

impl From<scry_core::Permission> for Permission {
    fn from(value: scry_core::Permission) -> Self {
        Self {
            prefix: value.prefix,
            with_glob: value.with_glob,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PermissionState {
    Allow,
    Deny,
    Error,
}

impl From<scry_core::PermissionState> for PermissionState {
    fn from(value: scry_core::PermissionState) -> Self {
        match value {
            scry_core::PermissionState::Allow => Self::Allow,
            scry_core::PermissionState::Deny => Self::Deny,
            scry_core::PermissionState::Error => Self::Error,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
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

impl From<scry_core::UserDecision> for UserDecision {
    fn from(value: scry_core::UserDecision) -> Self {
        match value {
            scry_core::UserDecision::AllowOnce { call_id } => Self::AllowOnce { call_id },
            scry_core::UserDecision::Allow {
                call_id,
                command,
                glob,
            } => Self::Allow {
                call_id,
                command,
                glob,
            },
            scry_core::UserDecision::AllowSession {
                session_id,
                call_id,
            } => Self::AllowSession {
                session_id,
                call_id,
            },
            scry_core::UserDecision::IgnorePermission {
                session_id,
                call_id,
            } => Self::IgnorePermission {
                session_id,
                call_id,
            },
            scry_core::UserDecision::Deny { call_id } => Self::Deny { call_id },
        }
    }
}

impl From<UserDecision> for scry_core::UserDecision {
    fn from(value: UserDecision) -> Self {
        match value {
            UserDecision::AllowOnce { call_id } => Self::AllowOnce { call_id },
            UserDecision::Allow {
                call_id,
                command,
                glob,
            } => Self::Allow {
                call_id,
                command,
                glob,
            },
            UserDecision::AllowSession {
                session_id,
                call_id,
            } => Self::AllowSession {
                session_id,
                call_id,
            },
            UserDecision::IgnorePermission {
                session_id,
                call_id,
            } => Self::IgnorePermission {
                session_id,
                call_id,
            },
            UserDecision::Deny { call_id } => Self::Deny { call_id },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionListItem {
    pub session_id: Uuid,
    pub title: String,
    pub last_update: i64,
}

impl From<scry_core::SessionListItem> for SessionListItem {
    fn from(value: scry_core::SessionListItem) -> Self {
        Self {
            session_id: value.session_id,
            title: value.title,
            last_update: value.last_update,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<String>,
}

/// Mirror of core's `ProviderStatis` (renamed to fix the typo at the boundary).
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProviderStatus {
    pub models: Vec<Model>,
    pub status: HealthStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorConnection {
    pub preferred: bool,
    pub prefer_model: String,
    pub prefer_effort: String,
    pub status: ProviderStatus,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Connector {
    pub id: ProviderId,
    pub connection: Option<ConnectorConnection>,
}

impl From<scry_core::Connector> for Connector {
    fn from(value: scry_core::Connector) -> Self {
        Self {
            id: value.id.into(),
            connection: value.connection.map(|connection| ConnectorConnection {
                preferred: connection.preferred,
                prefer_model: connection.prefer_model,
                prefer_effort: connection.prefer_effort,
                status: ProviderStatus {
                    models: connection
                        .status
                        .model
                        .into_iter()
                        .map(|model| Model {
                            id: model.id,
                            name: model.name,
                            default_reasoning_effort: model.default_reasoning_effort,
                            supported_reasoning_efforts: model.supported_reasoning_efforts,
                        })
                        .collect(),
                    status: connection.status.status.into(),
                    error: connection.status.error,
                },
            }),
        }
    }
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
                    |e| ScryError::InvalidArgument {
                        message: format!("invalid device-code transaction payload: {e}"),
                    },
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct Action {
    /// action name for both UI display and as action enum
    pub label: String,
    /// action input params
    pub params: Vec<String>,
    /// whether the action is the default action.
    pub primary: bool,
}

impl From<scry_core::Action> for Action {
    fn from(value: scry_core::Action) -> Self {
        Self {
            label: value.label,
            params: value.params,
            primary: value.primary,
        }
    }
}

impl From<Action> for scry_core::Action {
    fn from(value: Action) -> Self {
        Self {
            label: value.label,
            params: value.params,
            primary: value.primary,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ActionOutcome {
    Hide,
    Stay,
    Replace { input: String },
}

impl From<scry_core::ActionOutcome> for ActionOutcome {
    fn from(value: scry_core::ActionOutcome) -> Self {
        match value {
            scry_core::ActionOutcome::Hide => Self::Hide,
            scry_core::ActionOutcome::Stay => Self::Stay,
            scry_core::ActionOutcome::Replace { input } => Self::Replace { input },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
    Webp,
    Gif,
}

impl From<scry_core::ImageFormat> for ImageFormat {
    fn from(value: scry_core::ImageFormat) -> Self {
        match value {
            scry_core::ImageFormat::Png => Self::Png,
            scry_core::ImageFormat::Jpeg => Self::Jpeg,
            scry_core::ImageFormat::Svg => Self::Svg,
            scry_core::ImageFormat::Webp => Self::Webp,
            scry_core::ImageFormat::Gif => Self::Gif,
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
            scry_core::IconRef::Embedded { format, data } => Self::Embedded {
                format: format.into(),
                data,
            },
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
            actions: value.actions.into_iter().map(Into::into).collect(),
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
            scry_core::ChatRenderEvent::TextDelta { provider_id, text } => Self::TextDelta {
                provider_id: provider_id.into(),
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
                decisions: decisions.into_iter().map(Into::into).collect(),
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
