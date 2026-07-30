mod connect;
mod constant;
mod runtime;

use std::sync::Arc;

use paloma_provider_base::{
    ApiKeyConnector, Dispatcher, ProviderAuthenticator, ProviderCache, ProviderClient,
    ProviderError, ProviderRuntime, ProviderRuntimeService, Result, request_client,
};
use paloma_provider_protocol::v1::{Backend, ProviderAuth};
use paloma_utils::init_logging;

use crate::{
    connect::{CodexConnector, INSTRUCTION_URL},
    constant::{BACKENDS, PROVIDER_ID, backend_id},
    runtime::{CodexRuntime, OpenAIRuntime},
};

struct OpenAIGroup {
    openai: ApiKeyConnector,
    codex: CodexConnector,
}

impl OpenAIGroup {
    fn new(request: reqwest::Client) -> Self {
        Self {
            openai: ApiKeyConnector {
                backend_id: backend_id::OPENAI_API.into(),
                instructions_url: INSTRUCTION_URL,
            },
            codex: CodexConnector::new(request),
        }
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for OpenAIGroup {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn description(&self) -> &str {
        "OpenAI models through API key or ChatGPT subscription."
    }

    fn backends(&self) -> Vec<Backend> {
        BACKENDS.clone()
    }

    fn connector(&self, backend_id: &str) -> Option<&dyn ProviderAuthenticator> {
        match backend_id {
            backend_id::OPENAI_API => Some(&self.openai),
            backend_id::CODEX => Some(&self.codex),
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
            backend_id::OPENAI_API => {
                Arc::new(OpenAIRuntime::new(auth, request.clone(), Arc::clone(cache)).await)
            },
            backend_id::CODEX => Arc::new(
                CodexRuntime::new(auth, request.clone(), Arc::clone(cache), dispatcher.clone())
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
            ProviderRuntimeService::new(OpenAIGroup::new(request.clone()), request)
                .serve()
                .await
        })
}

#[allow(dead_code)]
#[tokio::main]
async fn main() -> Result<()> {
    init_logging("info".into());
    let request = request_client()?;
    ProviderRuntimeService::new(OpenAIGroup::new(request.clone()), request)
        .serve()
        .await
}
