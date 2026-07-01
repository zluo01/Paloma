use std::sync::{
    RwLock,
    atomic::{AtomicU8, Ordering},
};

use log::error;
use tokio::sync::Mutex;

use super::shared::{
    ClaudeAuth, RESPONSES_URL, build_request_body, fetch_models, parse_stream_error,
    response_event_stream,
};
use crate::{
    entity::{HealthStatus, ProviderId},
    provider::{
        Auth, ChatRequest, ChatStream, Model, ProviderClient, Result,
        runtime::{AvailableModels, unix_now},
    },
};

pub struct AnthropicRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicU8,
    error: RwLock<Option<String>>,
    models: Mutex<Option<AvailableModels>>,
}

impl AnthropicRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client) -> Self {
        let api_key = match credential {
            Auth::ApiKey(api_key) => api_key.trim().to_string(),
            Auth::OAuth { .. } => {
                return Self::unhealthy(
                    request,
                    "Anthropic API provider requires api_key credentials".into(),
                );
            },
        };

        if api_key.is_empty() {
            return Self::unhealthy(request, "Anthropic API key is required".into());
        }

        match fetch_models(&request, ClaudeAuth::ApiKey(&api_key)).await {
            Ok(models) => Self {
                request,
                api_key,
                status: AtomicU8::new(HealthStatus::Running as u8),
                error: RwLock::new(None),
                models: Mutex::new(Some(models)),
            },
            Err(e) => Self::unhealthy(request, format!("fail to connect to Anthropic: {e}")),
        }
    }

    fn unhealthy(request: reqwest::Client, error_msg: String) -> Self {
        Self {
            request,
            api_key: String::new(),
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: RwLock::new(Some(error_msg)),
            models: Mutex::new(None),
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
        let mut cache = self.models.lock().await;

        // Serve the cached catalogue while it's still fresh.
        if let Some(cached) = cache.as_ref()
            && unix_now() < cached.expires_at
        {
            return Some(cached.models.clone());
        }

        match fetch_models(&self.request, ClaudeAuth::ApiKey(&self.api_key)).await {
            Ok(result) => {
                let models = result.models.clone();
                *cache = Some(result);
                Some(models)
            },
            Err(e) => {
                error!("failed to refresh claude model catalogue: {e}");
                cache.as_ref().map(|cached| cached.models.clone())
            },
        }
    }

    fn health_statue(&self) -> HealthStatus {
        HealthStatus::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn error(&self) -> Option<String> {
        self.error.read().unwrap().clone()
    }
}
