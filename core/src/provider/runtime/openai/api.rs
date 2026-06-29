use std::sync::{
    RwLock,
    atomic::{AtomicU8, Ordering},
};

use log::debug;

use super::shared::{OPENAI_MODEL_CATALOG, build_request_body, response_event_stream};
use crate::{
    entity::{HealthStatus, ProviderId},
    provider::{Auth, ChatRequest, ChatStream, Model, ProviderClient, Result},
};

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const MODELS_URL: &str = "https://api.openai.com/v1/models";

pub struct OpenAIRuntime {
    request: reqwest::Client,
    api_key: String,
    status: AtomicU8,
    error: RwLock<Option<String>>,
}

impl OpenAIRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client) -> Self {
        let api_key = match credential {
            Auth::ApiKey(api_key) => api_key.trim().to_string(),
            Auth::OAuth { .. } => {
                return Self::unhealthy(
                    request,
                    "OpenAI API provider requires api_key credentials".into(),
                );
            },
        };

        if api_key.is_empty() {
            return Self::unhealthy(request, "OpenAI API key is required".into());
        }

        match health_check(&request, &api_key).await {
            Ok(()) => Self {
                request,
                api_key,
                status: AtomicU8::new(HealthStatus::Running as u8),
                error: RwLock::new(None),
            },
            Err(e) => Self::unhealthy(request, format!("fail to connect to openai: {e}")),
        }
    }

    fn unhealthy(request: reqwest::Client, error_msg: String) -> Self {
        Self {
            request,
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
            .await?
            .error_for_status()?;

        Ok(response_event_stream(response, ProviderId::OpenAI))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        Some(OPENAI_MODEL_CATALOG.clone())
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
        .get(MODELS_URL)
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
                crate::provider::ProviderError::Other("OpenAI API health check fails.".into())
            })?;
        Err(crate::provider::ProviderError::Other(message.to_string()))
    }
}
