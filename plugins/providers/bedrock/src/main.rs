mod connect;
mod constant;
mod runtime;

use std::sync::Arc;

use paloma_provider_base::{
    Dispatcher, ProviderAuthenticator, ProviderCache, ProviderClient, ProviderError,
    ProviderRuntime, ProviderRuntimeService, Result, request_client,
};
use paloma_provider_protocol::{v1 as proto, v1::ProviderAuth};
use paloma_utils::init_logging;

use crate::{
    connect::BedrockConnector,
    constant::{BACKENDS, PROVIDER_ID, backend_id},
    runtime::BedrockRuntime,
};

struct BedrockGroup {
    bedrock: BedrockConnector,
}

#[async_trait::async_trait]
impl ProviderRuntime for BedrockGroup {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn description(&self) -> &str {
        "Foundation models on Amazon Bedrock through the Converse API."
    }

    fn backends(&self) -> Vec<proto::Backend> {
        BACKENDS.clone()
    }

    fn connector(&self, backend_id: &str) -> Option<&dyn ProviderAuthenticator> {
        match backend_id {
            backend_id::BEDROCK_API => Some(&self.bedrock),
            _ => None,
        }
    }

    async fn build_runtime(
        &self,
        backend_id: &str,
        auth: &ProviderAuth,
        _request: &reqwest::Client,
        cache: &Arc<ProviderCache>,
        _dispatcher: &Dispatcher,
    ) -> Result<Arc<dyn ProviderClient>> {
        Ok(match backend_id {
            backend_id::BEDROCK_API => Arc::new(BedrockRuntime::new(auth, Arc::clone(cache)).await),
            id => {
                return Err(ProviderError::Other(format!(
                    "unknown backend {id}. This indicates a bug."
                )));
            },
        })
    }
}

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    init_logging("info".into());
    let request = request_client()?;
    ProviderRuntimeService::new(
        BedrockGroup {
            bedrock: BedrockConnector,
        },
        request,
    )
    .serve()
    .await
}
