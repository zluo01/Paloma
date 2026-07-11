mod cache;
mod future;
mod gated;
mod mcp;
mod oauth;
mod retry;
mod spill;
mod xml;

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use cache::ProviderCache;
pub(crate) use future::CompletableFuture;
pub(crate) use gated::Gated;
pub(crate) use mcp::mcp_function_name_encode;
pub use oauth::OAuthCallbackState;
pub(crate) use oauth::{OAuthError, finalize_oauth_connection, init_oauth_connection};
pub(crate) use retry::attempt_with_retry;
pub(crate) use spill::write_spill_file;
pub(crate) use xml::Element;

/// unix epoch in seconds.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
