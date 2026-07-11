use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use log::error;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::shared::{
    ClaudeAuth, RESPONSES_URL, build_request_body, fetch_models, parse_stream_error,
    response_event_stream,
};
use crate::{
    db::{AuthKind, Storage},
    entity::{HealthStatus, ProviderId},
    provider::{
        Auth, ChatRequest, ChatStream, Model, ProviderClient, Result,
        runtime::{RefreshRequest, cached_models},
    },
    utils::{ProviderCache, unix_now},
};

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const CLAUDE_CODE_BETA_HEADER: &str =
    "claude-code-20250219,oauth-2025-04-20,structured-outputs-2025-11-13";

pub struct ClaudeRuntime {
    request: reqwest::Client,
    storage: Storage,
    provider_cache: Arc<ProviderCache>,
    token_state: RwLock<TokenState>,
    refresh_lock: Mutex<()>,
    status: AtomicU8,
    error: RwLock<Option<String>>,
}

impl ClaudeRuntime {
    pub async fn new(
        credential: &Auth,
        request: reqwest::Client,
        storage: Storage,
        provider_cache: Arc<ProviderCache>,
    ) -> Self {
        let auth = match credential {
            Auth::OAuth {
                refresh_token: Some(_),
                ..
            } => credential.clone(),
            Auth::OAuth {
                refresh_token: None,
                ..
            } => {
                return Self::unhealthy(
                    request,
                    storage,
                    provider_cache,
                    "Claude credential is missing a refresh_token".into(),
                );
            },
            Auth::ApiKey(_) => {
                return Self::unhealthy(
                    request,
                    storage,
                    provider_cache,
                    "Claude does not support api_key credentials".into(),
                );
            },
        };

        match fetch_access_token(&request, &auth, &storage).await {
            Ok((access_token, auth)) => {
                let warmup = provider_cache
                    .models(ProviderId::Anthropic, || {
                        fetch_models(&request, ClaudeAuth::AccessToken(&access_token))
                    })
                    .await;
                match warmup {
                    Ok(_) => Self {
                        request,
                        token_state: RwLock::new(TokenState { auth, access_token }),
                        refresh_lock: Mutex::new(()),
                        storage,
                        provider_cache,
                        status: AtomicU8::new(HealthStatus::Running as u8),
                        error: RwLock::new(None),
                    },
                    Err(e) => {
                        error!("fail to fetch models on initialization. {e}");
                        Self::unhealthy(
                            request,
                            storage,
                            provider_cache,
                            format!("fail to connect to Claude Code: {e}"),
                        )
                    },
                }
            },
            Err(e) => {
                error!("fail to refresh for access token on initialization. {e}");
                Self::unhealthy(
                    request,
                    storage,
                    provider_cache,
                    format!("fail to connect to Claude Code: {e}"),
                )
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(
        request: reqwest::Client,
        storage: Storage,
        provider_cache: Arc<ProviderCache>,
        error_msg: String,
    ) -> Self {
        Self {
            request,
            token_state: RwLock::new(TokenState {
                auth: Auth::OAuth {
                    refresh_token: None,
                    expires_at: None,
                },
                access_token: String::new(),
            }),
            refresh_lock: Mutex::new(()),
            storage,
            provider_cache,
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: RwLock::new(Some(error_msg)),
        }
    }

    /// Flag the runtime unhealthy and record `error` for status reporting.
    fn mark_unhealthy(&self, error: String) {
        self.status
            .store(HealthStatus::Unhealthy as u8, Ordering::Relaxed);
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
        let current_auth = self.token_state.read().unwrap().auth.clone();
        let (new_tokens, auth) =
            fetch_access_token(&self.request, &current_auth, &self.storage).await?;
        *self.token_state.write().unwrap() = TokenState {
            auth,
            access_token: new_tokens.clone(),
        };
        Ok(new_tokens)
    }

    fn cached_access_token(&self) -> Option<String> {
        let state = self.token_state.read().unwrap();
        let valid_until = match state.auth {
            Auth::OAuth {
                expires_at: Some(expires_at),
                ..
            // 5-minute margin before the token actually expires.
            } => expires_at.saturating_sub(Duration::from_mins(5).as_secs()),
            _ => 0,
        };
        let now = unix_now();
        if now <= valid_until {
            return Some(state.access_token.clone());
        }
        None
    }
}

#[async_trait::async_trait]
impl ProviderClient for ClaudeRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::ClaudeCode
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let token = self
            .refresh()
            .await
            .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

        let body = build_request_body(&request, ProviderId::ClaudeCode);

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

        Ok(response_event_stream(response, ProviderId::ClaudeCode))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        cached_models(&self.provider_cache, ProviderId::Anthropic, || async move {
            let token = self
                .refresh()
                .await
                .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;
            fetch_models(&self.request, ClaudeAuth::AccessToken(&token)).await
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

async fn fetch_access_token(
    request: &reqwest::Client,
    auth: &Auth,
    storage: &Storage,
) -> Result<(String, Auth)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&RefreshRequest::new(auth, CLIENT_ID)?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    storage
        .update_provider(
            &ProviderId::ClaudeCode,
            &AuthKind::Oauth,
            &response.refresh_token,
        )
        .await?;

    // get the epoch time current access token will expire
    let expires_at = unix_now() + response.expires_in;

    Ok((
        response.access_token,
        Auth::OAuth {
            refresh_token: Some(response.refresh_token),
            expires_at: Some(expires_at),
        },
    ))
}

#[derive(Clone)]
struct TokenState {
    auth: Auth,
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
