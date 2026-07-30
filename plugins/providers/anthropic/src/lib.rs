mod connect;
mod constant;
mod runtime;

use std::sync::Arc;

use paloma_provider_base::{
    ApiKeyConnector, Dispatcher, ProviderAuthenticator, ProviderCache, ProviderClient,
    ProviderError, ProviderRuntime, ProviderRuntimeService, Result, request_client,
};
use paloma_provider_protocol::{v1 as proto, v1::ProviderAuth};
use paloma_utils::init_logging;

use crate::{
    connect::{ClaudeCodeConnector, INSTRUCTION_URL},
    constant::{BACKENDS, PROVIDER_ID, backend_id},
    runtime::{AnthropicRuntime, ClaudeRuntime},
};

struct AnthropicGroup {
    anthropic: ApiKeyConnector,
    claude_code: ClaudeCodeConnector,
}

impl AnthropicGroup {
    fn new(request: reqwest::Client) -> Self {
        Self {
            anthropic: ApiKeyConnector {
                backend_id: backend_id::ANTHROPIC_API.into(),
                instructions_url: INSTRUCTION_URL,
            },
            claude_code: ClaudeCodeConnector::new(request),
        }
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for AnthropicGroup {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn description(&self) -> &str {
        "Anthropic models through API key or Claude subscription."
    }

    fn backends(&self) -> Vec<proto::Backend> {
        BACKENDS.clone()
    }

    fn connector(&self, backend_id: &str) -> Option<&dyn ProviderAuthenticator> {
        match backend_id {
            backend_id::ANTHROPIC_API => Some(&self.anthropic),
            backend_id::CLAUDE_CODE => Some(&self.claude_code),
            _ => None,
        }
    }

    async fn build_runtime(
        &self,
        backend_id: &str,
        auth: &ProviderAuth,
        request: &reqwest::Client,
        cache: &Arc<ProviderCache>,
        dispatcher: &Dispatcher,
    ) -> Result<Arc<dyn ProviderClient>> {
        Ok(match backend_id {
            backend_id::ANTHROPIC_API => {
                Arc::new(AnthropicRuntime::new(auth, request.clone(), Arc::clone(cache)).await)
            },
            backend_id::CLAUDE_CODE => Arc::new(
                ClaudeRuntime::new(auth, request.clone(), Arc::clone(cache), dispatcher.clone())
                    .await,
            ),
            id => {
                return Err(ProviderError::Other(format!(
                    "unknown backend {id}. This indicates a bug."
                )));
            },
        })
    }
}

pub fn run() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            init_logging("info".into());
            let request = request_client()?;
            ProviderRuntimeService::new(AnthropicGroup::new(request.clone()), request)
                .serve()
                .await
        })
}

#[allow(dead_code)]
#[tokio::main]
async fn main() -> Result<()> {
    init_logging("info".into());
    let request = request_client()?;
    ProviderRuntimeService::new(AnthropicGroup::new(request.clone()), request)
        .serve()
        .await
}
