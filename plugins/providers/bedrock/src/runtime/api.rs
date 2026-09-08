use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};

use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
use log::error;
use paloma_provider_base::{
    Dispatcher, ProviderCache, ProviderClient, ProviderError, Result, SSE_IDLE_TIMEOUT,
    cached_models,
};
use paloma_provider_protocol::v1::{
    ChatRequest, Done, Model, ProviderAuth, ProviderHealthStatus, chat_response,
    provider_auth::Payload,
};

use super::{
    models::fetch_models,
    stream::{StreamStep, on_stream_event},
    utils::parse_error,
};
use crate::{
    connect::BedrockCredential,
    constant::backend_id,
    runtime::codec::{
        construct_messages, construct_reasoning_config, construct_system_prompt,
        construct_tool_config,
    },
};

pub struct BedrockRuntime {
    runtime_client: aws_sdk_bedrockruntime::Client,
    control_client: aws_sdk_bedrock::Client,
    status: AtomicI32,
    provider_cache: Arc<ProviderCache>,
    error: RwLock<Option<String>>,
}

impl BedrockRuntime {
    pub async fn new(credential: &ProviderAuth, provider_cache: Arc<ProviderCache>) -> Self {
        let credential = match credential.payload.as_ref() {
            Some(Payload::ApiKey(credential)) => credential.trim().to_string(),
            Some(Payload::RefreshToken(_)) | None => {
                return Self::unhealthy(
                    provider_cache,
                    "Amazon Bedrock backend requires API key credentials. This indicates a bug."
                        .into(),
                );
            },
        };

        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
        // if credential exists, we favor to use long term api key instead of IAM role
        if !credential.is_empty() {
            let BedrockCredential { region, api_key } = match BedrockCredential::parse(&credential)
            {
                Ok(credential) => credential,
                Err(e) => return Self::unhealthy(provider_cache, e.to_string()),
            };
            loader = loader
                .region(aws_config::Region::new(region))
                .token_provider(aws_sdk_bedrockruntime::config::Token::new(api_key, None))
                // following line make sure sigV4 always honor the api key and not rely on local aws config
                .auth_scheme_preference([HTTP_BEARER_AUTH_SCHEME_ID]);
        }
        let sdk_config = loader.load().await;
        let runtime_client = aws_sdk_bedrockruntime::Client::new(&sdk_config);
        let control_client = aws_sdk_bedrock::Client::new(&sdk_config);

        // call fetch model explicitly to verify credential validness, then cache the models
        match fetch_models(&control_client).await {
            Ok(models) => {
                provider_cache
                    .insert_models(backend_id::BEDROCK_API.into(), models)
                    .await;
                Self {
                    runtime_client,
                    control_client,
                    status: AtomicI32::new(ProviderHealthStatus::Running as i32),
                    provider_cache,
                    error: RwLock::new(None),
                }
            },
            Err(e) => Self::unhealthy(
                provider_cache,
                format!("fail to connect to Amazon Bedrock: {e}"),
            ),
        }
    }

    fn unhealthy(provider_cache: Arc<ProviderCache>, error_msg: String) -> Self {
        let placeholder = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();
        Self {
            runtime_client: aws_sdk_bedrockruntime::Client::new(&placeholder),
            control_client: aws_sdk_bedrock::Client::new(&placeholder),
            status: AtomicI32::new(ProviderHealthStatus::Unhealthy as i32),
            provider_cache,
            error: RwLock::new(Some(error_msg)),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for BedrockRuntime {
    fn id(&self) -> String {
        backend_id::BEDROCK_API.into()
    }

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()> {
        let output = self
            .runtime_client
            .converse_stream()
            .model_id(&request.model)
            .set_system(Some(construct_system_prompt(&request)))
            .set_messages(Some(construct_messages(&request)?))
            .set_tool_config(construct_tool_config(&request)?)
            .set_additional_model_request_fields(construct_reasoning_config(&request))
            .send()
            .await
            .map_err(|e| ProviderError::Other(parse_error(&e)))?;

        let mut stream = output.stream;
        let mut placeholder = None;
        loop {
            let next = match tokio::time::timeout(SSE_IDLE_TIMEOUT, stream.recv()).await {
                Err(_) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(format!(
                            "converse stream idle timeout: no activity for {SSE_IDLE_TIMEOUT:?}"
                        )))
                        .await;
                    return Ok(());
                },
                Ok(Err(e)) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(format!(
                            "converse stream error: {}",
                            parse_error(&e)
                        )))
                        .await;
                    return Ok(());
                },
                Ok(Ok(None)) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(
                            "converse stream ended without a message stop event".into(),
                        ))
                        .await;
                    return Ok(());
                },
                Ok(Ok(Some(event))) => event,
            };

            match on_stream_event(&next, &mut placeholder) {
                Ok(StreamStep::Continue(None)) => {},
                Ok(StreamStep::Continue(Some(payload))) => {
                    dispatcher.send_chat_event(payload).await;
                },
                Ok(StreamStep::Item(item)) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::OutputItem(item))
                        .await;
                },
                Ok(StreamStep::Done) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Done(Done {}))
                        .await;
                    return Ok(());
                },
                Err(e) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(e.to_string()))
                        .await;
                    return Ok(());
                },
            }
        }
    }

    async fn models(self: Arc<Self>) -> Option<Vec<Model>> {
        let cache = Arc::clone(&self.provider_cache);

        cached_models(&cache, backend_id::BEDROCK_API.into(), move || async move {
            fetch_models(&self.control_client).await
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
