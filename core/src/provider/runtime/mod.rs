mod claude;
mod openai;

use std::time::Duration;

pub use claude::{AnthropicRuntime, ClaudeRuntime};
pub use openai::{CodexRuntime, OpenAIRuntime};
use serde::Serialize;

use crate::provider::{Auth, Model, ProviderError};

/// How long a fetched model catalogue is served from cache before a refetch.
const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(1).as_secs();

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

struct AvailableModels {
    models: Vec<Model>,
    expires_at: u64,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

impl<'a> RefreshRequest<'a> {
    /// Build the refresh-grant body from a stored OAuth credential, failing
    /// if it carries no refresh token.
    fn new(auth: &'a Auth, client_id: &'a str) -> crate::provider::Result<Self> {
        let Auth::OAuth {
            refresh_token: Some(refresh_token),
            ..
        } = auth
        else {
            return Err(ProviderError::Other("missing a refresh_token".into()));
        };
        Ok(Self {
            client_id,
            grant_type: "refresh_token",
            refresh_token,
        })
    }
}
