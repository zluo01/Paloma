use std::time::Duration;

use log::error;

mod api_connector;
mod cache;
mod codec;
mod constants;
mod dispatcher;
mod entity;
mod error;
mod service;
mod traits;

pub use api_connector::ApiKeyConnector;
pub use cache::ProviderCache;
pub use codec::{
    ProviderDecoder, ProviderEncoder, ProviderMeta, provider_meta, provider_meta_to_map,
};
pub use constants::ENVIRONMENT_CONTEXT;
pub use dispatcher::Dispatcher;
pub use entity::{OAuthState, RefreshRequest};
use scry_provider_protocol::v1::Model;
pub use service::{ProviderRuntime, ProviderRuntimeService};
pub use traits::{ProviderAuthenticator, ProviderClient, ProviderService};

pub use crate::error::{ProviderError, Result};

pub const SSE_IDLE_TIMEOUT: Duration = Duration::from_mins(5);

pub fn request_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(900))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(5)
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Some(Duration::from_secs(60)))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        .build()?)
}

pub async fn cached_models<F, Fut>(
    cache: &ProviderCache,
    id: String,
    fetch: F,
) -> Option<Vec<Model>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<Model>>> + Send + 'static,
{
    match cache.models(id.clone(), fetch).await {
        Ok(models) => Some(models),
        Err(e) => {
            error!("failed to refresh {id} model catalogue: {e}");
            None
        },
    }
}
