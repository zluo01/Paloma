use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU8, Ordering},
};

use log::{debug, error};

use super::shared::{
    ModelsResponse, build_request_body, models_from_response, parse_stream_error,
    response_event_stream,
};
use crate::{
    entity::{HealthStatus, ProviderId},
    provider::{
        Auth, ChatRequest, ChatStream, Model, ProviderClient, ProviderError, Result,
        runtime::cached_models,
    },
    utils::{ProviderCache, attempt_with_retry},
};

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const HEALTH_CHECK_URL: &str = "https://api.openai.com/v1/models";
const MODELS_URL: &str = "https://raw.githubusercontent.com/openai/codex/refs/heads/main/codex-rs/models-manager/models.json";

pub struct OpenAIRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicU8,
    provider_cache: Arc<ProviderCache>,
    error: RwLock<Option<String>>,
}

impl OpenAIRuntime {
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
                    "OpenAI API provider requires api_key credentials".into(),
                );
            },
        };

        if api_key.is_empty() {
            return Self::unhealthy(request, provider_cache, "OpenAI API key is required".into());
        }

        match health_check(&request, &api_key).await {
            Ok(()) => match provider_cache
                .models(ProviderId::OpenAI, || fetch_models(&request))
                .await
            {
                Ok(_) => Self {
                    request,
                    provider_cache,
                    api_key,
                    status: AtomicU8::new(HealthStatus::Running as u8),
                    error: RwLock::new(None),
                },
                Err(e) => {
                    error!("fail to fetch models on initialization. {e}");
                    Self::unhealthy(
                        request,
                        provider_cache,
                        format!("fail to fetch OpenAI model catalogue: {e}"),
                    )
                },
            },
            Err(e) => Self::unhealthy(
                request,
                provider_cache,
                format!("fail to connect to openai: {e}"),
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
            provider_cache,
            api_key: String::new(),
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: RwLock::new(Some(error_msg)),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for OpenAIRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAI
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let body = build_request_body(&request, ProviderId::OpenAI);

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&self.api_key)
            .header(reqwest::header::USER_AGENT, "scry-openai")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(parse_stream_error(&body));
        }

        Ok(response_event_stream(response, ProviderId::OpenAI))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        cached_models(&self.provider_cache, ProviderId::OpenAI, || {
            fetch_models(&self.request)
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

async fn health_check(request: &reqwest::Client, api_key: &str) -> Result<()> {
    let response = request
        .get(HEALTH_CHECK_URL)
        .bearer_auth(api_key)
        .header(reqwest::header::USER_AGENT, "scry-openai")
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::OK {
        Ok(())
    } else {
        let response: serde_json::Value = response.json().await?;
        let message = response
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                debug!("fail to get error for health check response. {}", response);
                ProviderError::Other("OpenAI API health check fails.".into())
            })?;
        Err(ProviderError::Other(message.to_string()))
    }
}

async fn fetch_models(request: &reqwest::Client) -> Result<Vec<Model>> {
    let response: ModelsResponse = attempt_with_retry(
        || async move {
            Ok(request
                .get(MODELS_URL)
                .header(reqwest::header::USER_AGENT, "scry")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?)
        },
        ProviderError::is_retryable,
    )
    .await?;

    Ok(models_from_response(response))
}
