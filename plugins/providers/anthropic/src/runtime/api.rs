use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};

use log::error;
use paloma_provider_base::{Dispatcher, ProviderCache, ProviderClient, Result, cached_models};
use paloma_provider_protocol::v1::{
    ChatRequest, Model, ProviderAuth, ProviderHealthStatus, provider_auth::Payload,
};

use super::shared::{
    ClaudeAuth, RESPONSES_URL, build_request_body, fetch_models, parse_stream_error,
    response_event_stream,
};
use crate::constant::backend_id;

pub struct AnthropicRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicI32,
    provider_cache: Arc<ProviderCache>,
    error: RwLock<Option<String>>,
}

impl AnthropicRuntime {
    pub async fn new(
        credential: &ProviderAuth,
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
    ) -> Self {
        let api_key = match credential.payload.as_ref() {
            Some(Payload::ApiKey(api_key)) => api_key.trim().to_string(),
            Some(Payload::RefreshToken(_)) | None => {
                return Self::unhealthy(
                    request,
                    provider_cache,
                    "Anthropic API backend requires API key credentials. This indicate a bug."
                        .into(),
                );
            },
        };

        if api_key.is_empty() {
            return Self::unhealthy(
                request,
                provider_cache,
                "Anthropic API key is required".into(),
            );
        }

        // call fetch model explicitly to verify api key validness, then cache the models
        match fetch_models(&request, ClaudeAuth::ApiKey(&api_key)).await {
            Ok(models) => {
                provider_cache
                    .insert_models(backend_id::ANTHROPIC_API.into(), models)
                    .await;
                Self {
                    request,
                    api_key,
                    status: AtomicI32::new(ProviderHealthStatus::Running as i32),
                    provider_cache,
                    error: RwLock::new(None),
                }
            },
            Err(e) => Self::unhealthy(
                request,
                provider_cache,
                format!("fail to connect to Anthropic: {e}"),
            ),
        }
    }

    fn unhealthy(
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
        error_msg: String,
    ) -> Self {
        Self {
            request,
            api_key: String::new(),
            status: AtomicI32::new(ProviderHealthStatus::Unhealthy as i32),
            provider_cache,
            error: RwLock::new(Some(error_msg)),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for AnthropicRuntime {
    fn id(&self) -> String {
        backend_id::ANTHROPIC_API.into()
    }

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()> {
        let body = build_request_body(&request, backend_id::ANTHROPIC_API.into());

        let response = self
            .request
            .post(RESPONSES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(parse_stream_error(&body));
        }

        response_event_stream(response, backend_id::ANTHROPIC_API.into(), dispatcher).await;
        Ok(())
    }

    async fn models(self: Arc<Self>) -> Option<Vec<Model>> {
        let cache = Arc::clone(&self.provider_cache);

        cached_models(&cache, backend_id::ANTHROPIC_API.into(), move || async move {
            fetch_models(&self.request, ClaudeAuth::ApiKey(&self.api_key)).await
        })
        .await
    }

    fn health_status(&self) -> ProviderHealthStatus {
        let raw = self.status.load(Ordering::Acquire);
        ProviderHealthStatus::try_from(raw).unwrap_or_else(|_| {
            error!("unknown health status value {raw}. This indicates a bug.");
            ProviderHealthStatus::Unhealthy
        })
    }

    fn error(&self) -> Option<String> {
        self.error.read().unwrap().clone()
    }
}
