use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};

use log::{debug, error};
use scry_provider_base::{
    Dispatcher, ProviderCache, ProviderClient, ProviderError, Result, cached_models,
};
use scry_provider_protocol::v1::{
    ChatRequest, Model, ProviderAuth, ProviderHealthStatus, provider_auth::Payload,
};
use scry_utils::attempt_with_retry;

use crate::{
    constant::backend_id,
    runtime::shared::{
        MODELS_FETCH_TIMEOUT, ModelsResponse, build_request_body, models_from_response,
        parse_stream_error, response_event_stream,
    },
};

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const HEALTH_CHECK_URL: &str = "https://api.openai.com/v1/models";
const MODELS_URL: &str = "https://raw.githubusercontent.com/openai/codex/refs/heads/main/codex-rs/models-manager/models.json";

pub struct OpenAIRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicI32,
    provider_cache: Arc<ProviderCache>,
    error: RwLock<Option<String>>,
}

impl OpenAIRuntime {
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
                    "OpenAI API backend requires API key credentials. This indicate a bug.".into(),
                );
            },
        };

        if api_key.is_empty() {
            return Self::unhealthy(request, provider_cache, "OpenAI API key is required".into());
        }

        match health_check(&request, &api_key).await {
            Ok(()) => match provider_cache
                .models(backend_id::OPENAI_API.into(), || fetch_models(&request))
                .await
            {
                Ok(_) => Self {
                    request,
                    provider_cache,
                    api_key,
                    status: AtomicI32::new(ProviderHealthStatus::Running as i32),
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
            status: AtomicI32::new(ProviderHealthStatus::Unhealthy as i32),
            error: RwLock::new(Some(error_msg)),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for OpenAIRuntime {
    fn id(&self) -> String {
        backend_id::OPENAI_API.into()
    }

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()> {
        let body = build_request_body(&request, backend_id::OPENAI_API.into());

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

        response_event_stream(response, backend_id::OPENAI_API.into(), dispatcher).await;
        Ok(())
    }

    async fn models(&self) -> Option<Vec<Model>> {
        cached_models(&self.provider_cache, backend_id::OPENAI_API.into(), || {
            fetch_models(&self.request)
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
                .timeout(MODELS_FETCH_TIMEOUT)
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
