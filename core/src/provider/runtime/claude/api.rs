use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU8, Ordering},
};

use super::shared::{
    ClaudeAuth, RESPONSES_URL, build_request_body, fetch_models, parse_stream_error,
    response_event_stream,
};
use crate::{
    entity::{HealthStatus, ProviderId},
    provider::{
        Auth, ChatRequest, ChatStream, Model, ProviderClient, Result, runtime::cached_models,
    },
    utils::ProviderCache,
};

pub struct AnthropicRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicU8,
    provider_cache: Arc<ProviderCache>,
    error: RwLock<Option<String>>,
}

impl AnthropicRuntime {
    pub async fn new(
        credential: &Auth,
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
    ) -> Self {
        let api_key = match credential {
            Auth::ApiKey(api_key) => api_key.trim().to_string(),
            Auth::OAuth { .. } => {
                return Self::unhealthy(
                    request,
                    provider_cache,
                    "Anthropic API provider requires api_key credentials".into(),
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
                    .insert_models(ProviderId::Anthropic, models)
                    .await;
                Self {
                    request,
                    api_key,
                    status: AtomicU8::new(HealthStatus::Running as u8),
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
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            provider_cache,
            error: RwLock::new(Some(error_msg)),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for AnthropicRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let body = build_request_body(&request, ProviderId::Anthropic);

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

        Ok(response_event_stream(response, ProviderId::Anthropic))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        cached_models(&self.provider_cache, ProviderId::Anthropic, || {
            fetch_models(&self.request, ClaudeAuth::ApiKey(&self.api_key))
        })
        .await
    }

    fn health_statue(&self) -> HealthStatus {
        HealthStatus::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn error(&self) -> Option<String> {
        self.error.read().unwrap().clone()
    }
}
