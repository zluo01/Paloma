use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};

use log::error;
use scry_provider_base::{
    Dispatcher, OAuthState, ProviderCache, ProviderClient, ProviderError, RefreshRequest, Result,
    cached_models,
};
use scry_provider_protocol::v1::{
    AuthUpdateRequest, ChatRequest, Model, ProviderAuth, ProviderHealthStatus, response_event,
};
use scry_utils::unix_now;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::shared::{
    ClaudeAuth, RESPONSES_URL, build_request_body, fetch_models, parse_stream_error,
    response_event_stream,
};
use crate::constant::backend_id;

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const CLAUDE_CODE_BETA_HEADER: &str =
    "claude-code-20250219,oauth-2025-04-20,structured-outputs-2025-11-13";

pub struct ClaudeRuntime {
    request: reqwest::Client,
    provider_cache: Arc<ProviderCache>,
    token_state: RwLock<Option<TokenState>>,
    refresh_lock: Mutex<()>,
    status: AtomicI32,
    error: RwLock<Option<String>>,
    dispatcher: Dispatcher,
}

impl ClaudeRuntime {
    pub async fn new(
        credential: &ProviderAuth,
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
        dispatcher: Dispatcher,
    ) -> Self {
        let oauth = match OAuthState::try_from(credential) {
            Ok(oauth) => oauth,
            Err(e) => {
                return Self::unhealthy(
                    request,
                    provider_cache,
                    dispatcher,
                    format!("Claude: {e}"),
                );
            },
        };

        match fetch_access_token(&request, &oauth, &dispatcher).await {
            Ok((access_token, oauth)) => {
                let warmup = provider_cache
                    .models(backend_id::ANTHROPIC_API.into(), || {
                        fetch_models(&request, ClaudeAuth::AccessToken(&access_token))
                    })
                    .await;
                match warmup {
                    Ok(_) => Self {
                        request,
                        token_state: RwLock::new(Some(TokenState {
                            oauth,
                            access_token,
                        })),
                        refresh_lock: Mutex::new(()),
                        provider_cache,
                        status: AtomicI32::new(ProviderHealthStatus::Running as i32),
                        error: RwLock::new(None),
                        dispatcher,
                    },
                    Err(e) => {
                        error!("fail to fetch models on initialization. {e}");
                        Self::unhealthy(
                            request,
                            provider_cache,
                            dispatcher,
                            format!("fail to connect to Claude Code: {e}"),
                        )
                    },
                }
            },
            Err(e) => {
                error!("fail to refresh for access token on initialization. {e}");
                Self::unhealthy(
                    request,
                    provider_cache,
                    dispatcher,
                    format!("fail to connect to Claude Code: {e}"),
                )
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
        dispatcher: Dispatcher,
        error_msg: String,
    ) -> Self {
        Self {
            request,
            token_state: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            provider_cache,
            status: AtomicI32::new(ProviderHealthStatus::Unhealthy as i32),
            error: RwLock::new(Some(error_msg)),
            dispatcher,
        }
    }

    /// Flag the runtime unhealthy and record `error` for status reporting.
    fn mark_unhealthy(&self, error: String) {
        self.status
            .store(ProviderHealthStatus::Unhealthy as i32, Ordering::Release);
        *self.error.write().unwrap() = Some(error);
    }

    /// refresh access token if the current token is close to expired or already expired,
    /// otherwise return the current access token
    async fn refresh(&self) -> Result<String> {
        if let Some(token) = self.cached_access_token() {
            return Ok(token);
        }

        let _guard = self.refresh_lock.lock().await;

        if let Some(token) = self.cached_access_token() {
            return Ok(token);
        }

        // Expired (or about to): rotate the token and cache the new pair.
        let current_oauth = self
            .token_state
            .read()
            .unwrap()
            .as_ref()
            .map(|state| state.oauth.clone())
            .ok_or_else(|| ProviderError::Other("no oauth credential loaded".into()))?;
        let (new_tokens, oauth) =
            fetch_access_token(&self.request, &current_oauth, &self.dispatcher).await?;
        *self.token_state.write().unwrap() = Some(TokenState {
            oauth,
            access_token: new_tokens.clone(),
        });
        Ok(new_tokens)
    }

    fn cached_access_token(&self) -> Option<String> {
        let state = self.token_state.read().unwrap();
        let state = state.as_ref()?;
        state
            .oauth
            .is_fresh(unix_now())
            .then(|| state.access_token.clone())
    }
}

#[async_trait::async_trait]
impl ProviderClient for ClaudeRuntime {
    fn id(&self) -> String {
        backend_id::CLAUDE_CODE.into()
    }

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()> {
        let token = self
            .refresh()
            .await
            .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

        let body = build_request_body(&request, backend_id::CLAUDE_CODE.into());

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&token)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-beta", CLAUDE_CODE_BETA_HEADER)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(parse_stream_error(&body));
        }

        response_event_stream(response, backend_id::CLAUDE_CODE.into(), dispatcher).await;
        Ok(())
    }

    async fn models(&self) -> Option<Vec<Model>> {
        cached_models(
            &self.provider_cache,
            backend_id::ANTHROPIC_API.into(),
            || async move {
                let token = self
                    .refresh()
                    .await
                    .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;
                fetch_models(&self.request, ClaudeAuth::AccessToken(&token)).await
            },
        )
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

async fn fetch_access_token(
    request: &reqwest::Client,
    oauth: &OAuthState,
    dispatcher: &Dispatcher,
) -> Result<(String, OAuthState)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&RefreshRequest::new(oauth, CLIENT_ID))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // defensively rare case that try to update but job get canceled
    tokio::spawn({
        let dispatcher = dispatcher.clone();
        let refresh_token = response.refresh_token.clone();
        async move {
            dispatcher
                .send(response_event::Payload::AuthUpdateRequest(
                    AuthUpdateRequest {
                        backend_id: backend_id::CLAUDE_CODE.into(),
                        refresh_token,
                    },
                ))
                .await;
        }
    });

    // get the epoch time current access token will expire
    let expires_at = unix_now() + response.expires_in;

    Ok((
        response.access_token,
        OAuthState::rotated(response.refresh_token, expires_at),
    ))
}

#[derive(Clone)]
struct TokenState {
    oauth: OAuthState,
    access_token: String,
}

/*
{
    "token_type": "Bearer",
    "access_token": "ACCESS_TOKEN",
    "expires_in": 28800,
    "refresh_token": "REFRESH_TOKEN",
    "scope": "user:inference user:profile user:sessions:claude_code",
    "token_uuid": "TOKEN_UUID",
    "organization": {
        "uuid": "UUID",
        "name": "NAME"
    },
    "account": {
        "uuid": "UUID",
        "email_address": "EMAIL_ADDRESS"
    }
}
*/
#[derive(Debug, Deserialize)]
struct RefreshResponse {
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_in: u64,
    refresh_token: String,
    access_token: String,
}
