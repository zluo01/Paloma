mod claude;
mod openai;

use std::{future::Future, time::Duration};

pub use claude::{AnthropicRuntime, ClaudeRuntime};
use log::error;
pub use openai::{CodexRuntime, OpenAIRuntime};
use serde::Serialize;

use crate::{
    entity::ProviderId,
    provider::{Auth, Model, ProviderError, Result},
    utils::ProviderCache,
};

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

async fn cached_models<F, Fut>(
    cache: &ProviderCache,
    id: ProviderId,
    fetch: F,
) -> Option<Vec<Model>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<Model>>>,
{
    match cache.models(id, fetch).await {
        Ok(models) => Some(models),
        Err(e) => {
            error!("failed to refresh {id} model catalogue: {e}");
            None
        },
    }
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
    fn new(auth: &'a Auth, client_id: &'a str) -> Result<Self> {
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
