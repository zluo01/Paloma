pub type Result<T> = std::result::Result<T, ProviderError>;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON encode/decode failed: {0}")]
    Json(#[from] serde_json::Error),

    /// Process-fatal fault on the host<->plugin stdio transport. Never sent
    /// to the host as a wire-level error; only bubbles out of `serve`.
    #[error("plugin transport I/O failed: {0}")]
    Io(#[from] std::io::Error),

    /// Process-fatal fault on the host<->plugin stdio transport. Never sent
    /// to the host as a wire-level error; only bubbles out of `serve`.
    #[error("protobuf decode failed: {0}")]
    Decode(#[from] scry_provider_protocol::DecodeError),

    #[error("device authorization timed out after {0}s")]
    Timeout(u64),

    #[error("device poll failed: HTTP {status}: {body}")]
    PollFailed { status: u16, body: String },

    #[error("unexpected connection variant: expected {expected}")]
    InvalidConnection { expected: &'static str },

    /// Transient transport-layer failure (TCP reset mid-stream, TLS error,
    /// dropped HTTP/2 frame, etc.). A retry layer can re-issue the request
    /// with backoff and reasonably expect success. Distinct from
    /// `Other`/parse errors, which signal a logical fault and should not
    /// be retried.
    #[error("transport error: {0}")]
    Transport(String),

    #[error("{0}")]
    Other(String),
}

impl ProviderError {
    /// Whether a retry controller may safely re-issue the originating
    /// request. Currently only `Transport` (and `reqwest::Error`s that
    /// are themselves transport-level — connect / timeout) qualify.
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Transport(_) => true,
            ProviderError::Http(e) => e.is_connect() || e.is_timeout(),
            _ => false,
        }
    }
}

impl From<ProviderError> for scry_provider_protocol::v1::ProviderError {
    fn from(e: ProviderError) -> Self {
        Self {
            error: e.to_string(),
        }
    }
}
