use futures::stream::BoxStream;
use scry_storage::storage::Storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Result<T> = std::result::Result<T, ProviderError>;
pub type ChatStream = BoxStream<'static, Result<ChatEvent>>;

#[async_trait::async_trait]
pub trait ProviderClient: Send + Sync {
    fn id(&self) -> ProviderId;

    async fn refresh(&self, storage: &Storage) -> Result<Option<Auth>>;

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream>;

    async fn models(&self) -> Result<Vec<Model>>;
}

#[async_trait::async_trait]
pub trait ProviderAuthenticator: Send + Sync {
    fn id(&self) -> ProviderId;

    async fn init_connection(&self) -> Result<Connection>;

    async fn finalize_connection(&self, payload: Connection) -> Result<Auth>;
}

pub enum Connection {
    /// User opens a URL and types a code into the page.
    /// Codex, GitHub, Google device flow.
    DeviceCode {
        verification_uri: &'static str,
        user_code: String,
        transaction_payload: Value, // payload for finalized connection
    },
    /// User opens a URL; the browser redirects back to our local callback.
    /// Anthropic Claude, most "Sign in with X" web flows.
    BrowserRedirect {
        authorization_url: String, // long URL with PKCE challenge
    },
    /// User pastes a token/key from a settings page.
    /// Raw API keys (Anthropic API, OpenAI API directly).
    ManualInput {
        prompt: String, // "Enter your OpenAI API key"
        instructions_url: Option<String>,
    },
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Codex => "codex",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    ApiKey(String),
    OAuth {
        refresh_token: Option<String>,
        expires_in: Option<i64>,
    },
}

impl Auth {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Auth::ApiKey(_) => "api_key",
            Auth::OAuth { .. } => "oauth",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    TextDelta { text: String },
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub plan: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON encode/decode failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Storage(#[from] scry_storage::StorageError),

    #[error("device authorization timed out after {0}s")]
    Timeout(u64),

    #[error("device poll failed: HTTP {status}: {body}")]
    PollFailed { status: u16, body: String },

    #[error("unexpected connection variant: expected {expected}")]
    InvalidConnection { expected: &'static str },

    #[error("failed to parse timestamp {field}: {source}")]
    ParseTimestamp {
        field: &'static str,
        #[source]
        source: chrono::ParseError,
    },

    #[error("{0}")]
    Other(String),
}
