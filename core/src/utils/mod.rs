mod future;
mod gated;
mod mcp;
mod oauth;
mod spill;
mod xml;

use std::time::{SystemTime, UNIX_EPOCH};

pub use future::CompletableFuture;
pub(crate) use gated::Gated;
pub use mcp::mcp_function_name_encode;
pub use oauth::{OAuthCallbackState, OAuthError};
pub(crate) use oauth::{finalize_oauth_connection, init_oauth_connection};
pub use spill::write_spill_file;
pub use xml::Element;

/// unix epoch in seconds.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
