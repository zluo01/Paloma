mod claude;
mod openai;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use claude::AnthropicRuntime;
pub use openai::{CodexRuntime, OpenAIRuntime};

use crate::provider::Model;

/// How long a fetched model catalogue is served from cache before a refetch.
const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(1).as_secs();

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

struct AvailableModels {
    models: Vec<Model>,
    expires_at: u64,
}

/// unix epoch in seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
