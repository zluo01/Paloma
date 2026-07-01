use std::{
    sync::{
        RwLock,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use base64::Engine;
use log::error;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::shared::{
    ModelsResponse, build_request_body, models_from_response, parse_stream_error,
    response_event_stream,
};
use crate::{
    db::{AuthKind, Storage},
    entity::{HealthStatus, ProviderId},
    provider::{
        Auth, ChatRequest, ChatStream, Model, ProviderClient, ProviderError, Result,
        runtime::{AvailableModels, MODELS_CACHE_TTL_SECS, RefreshRequest, unix_now},
    },
};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// Codex CLI's own version, sent as `?client_version=` on `/backend-api/codex/models`.
/// The backend gates per-model availability on this via `minimal_client_version`
/// in each `ModelInfo`. Since we're impersonating Codex CLI, this must track
/// what real Codex CLI publishes, not our own package version.
///
/// Construction in Codex (major.minor.patch from `CARGO_PKG_VERSION`):
///   <https://github.com/openai/codex/blob/main/codex-rs/models-manager/src/lib.rs#L19-L26>
/// Query-param append:
///   <https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/models.rs#L35-L38>
/// Current published version (the source of truth — bump to match):
///   <https://www.npmjs.com/package/@openai/codex>
const CLIENT_VERSION: &str = "0.142.0";

pub struct CodexRuntime {
    request: reqwest::Client,
    storage: Storage,
    token_state: RwLock<TokenState>,
    refresh_lock: Mutex<()>,
    status: AtomicU8,
    error: RwLock<Option<String>>,
    models: Mutex<Option<AvailableModels>>,
}

impl CodexRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client, storage: Storage) -> Self {
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
                    "Codex credential is missing a refresh_token".into(),
                );
            },
            Auth::ApiKey(_) => {
                return Self::unhealthy(
                    request,
                    storage,
                    "Codex does not support api_key credentials".into(),
                );
            },
        };

        match fetch_access_token(&request, &auth, &storage).await {
            Ok((access_token, auth)) => match fetch_models(&request, &access_token).await {
                Ok(models) => Self {
                    request,
                    token_state: RwLock::new(TokenState { auth, access_token }),
                    refresh_lock: Mutex::new(()),
                    storage,
                    status: AtomicU8::new(HealthStatus::Running as u8),
                    error: RwLock::new(None),
                    models: Mutex::new(Some(models)),
                },
                Err(e) => {
                    error!("fail to fetch models on initialization. {e}");
                    Self::unhealthy(request, storage, format!("fail to connect to codex: {e}"))
                },
            },
            Err(e) => {
                error!("fail to refresh for access token on initialization. {e}");
                Self::unhealthy(request, storage, format!("fail to connect to codex: {e}"))
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(request: reqwest::Client, storage: Storage, error_msg: String) -> Self {
        Self {
            request,
            token_state: RwLock::new(TokenState {
                auth: Auth::OAuth {
                    refresh_token: None,
                    expires_at: None,
                },
                access_token: AccessToken {
                    access_token: String::new(),
                    chatgpt_account_id: String::new(),
                },
            }),
            refresh_lock: Mutex::new(()),
            storage,
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: RwLock::new(Some(error_msg)),
            models: Mutex::new(None),
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
    async fn refresh(&self) -> Result<AccessToken> {
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

    fn cached_access_token(&self) -> Option<AccessToken> {
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
impl ProviderClient for CodexRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let token = self
            .refresh()
            .await
            .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

        let body = build_request_body(&request, ProviderId::Codex);

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&token.access_token)
            .header("chatgpt-account-id", &token.chatgpt_account_id)
            .header("originator", "srcy")
            .header(reqwest::header::USER_AGENT, "scry-codex")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(parse_stream_error(&body));
        }

        Ok(response_event_stream(response, ProviderId::Codex))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        // Hold the lock across the refetch: concurrent callers single-flight
        // — the first refetches, the rest wait and pick up its cached result.
        let mut cache = self.models.lock().await;

        // Serve the cached catalogue while it's still fresh.
        if let Some(cached) = cache.as_ref()
            && unix_now() < cached.expires_at
        {
            return Some(cached.models.clone());
        }

        match self.refresh().await {
            Ok(access_token) => match fetch_models(&self.request, &access_token).await {
                Ok(result) => {
                    let models = result.models.clone();
                    *cache = Some(result);
                    Some(models)
                },
                Err(e) => {
                    error!("failed to refresh model catalogue: {e}");
                    cache.as_ref().map(|cached| cached.models.clone())
                },
            },
            Err(e) => {
                error!("failed to get access token to refresh model catalogue: {e}");
                self.mark_unhealthy(e.to_string());
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

async fn fetch_access_token(
    request: &reqwest::Client,
    auth: &Auth,
    storage: &Storage,
) -> Result<(AccessToken, Auth)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&RefreshRequest::new(auth, CLIENT_ID)?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let chatgpt_account_id = extract_chatgpt_account_id(&response.id_token)
        .ok_or_else(|| ProviderError::Other("id_token missing chatgpt_account_id claim".into()))?;

    // codex refresh token follows "rotate on use"
    // so we need to proactively update the db whenever we refresh.
    storage
        .update_provider(
            &ProviderId::Codex,
            &AuthKind::Oauth,
            &response.refresh_token,
        )
        .await?;

    // get the epoch time current access token will expire
    let expires_at = unix_now() + response.expires_in;

    Ok((
        AccessToken {
            access_token: response.access_token,
            chatgpt_account_id,
        },
        Auth::OAuth {
            refresh_token: Some(response.refresh_token),
            expires_at: Some(expires_at),
        },
    ))
}

/// Decode a JWT's middle segment (claims) and pull out
/// `https://api.openai.com/auth.chatgpt_account_id`.
///
/// No signature verification: we received this token over our own TLS exchange
/// with `auth.openai.com`, so the bytes can't have been tampered with in transit.
fn extract_chatgpt_account_id(id_token: &str) -> Option<String> {
    let payload_b64 = id_token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .map(str::to_string)
}

async fn fetch_models(request: &reqwest::Client, token: &AccessToken) -> Result<AvailableModels> {
    let url = format!("{MODELS_URL}?client_version={CLIENT_VERSION}");
    let response: ModelsResponse = request
        .get(&url)
        .bearer_auth(&token.access_token)
        .header("chatgpt-account-id", &token.chatgpt_account_id)
        .header("OpenAI-Beta", "responses=experimental")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let available_models = models_from_response(response);
    Ok(AvailableModels {
        models: available_models,
        expires_at: unix_now() + MODELS_CACHE_TTL_SECS,
    })
}

/// OAuth and access-token data derived from a refresh-token exchange. Held
/// together so refresh-token rotation and access-token replacement are atomic
/// from readers' perspective.
#[derive(Clone)]
struct TokenState {
    auth: Auth,
    access_token: AccessToken,
}

/// Access token data needed for Codex backend requests.
#[derive(Clone)]
struct AccessToken {
    access_token: String,
    chatgpt_account_id: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_in: u64,
    refresh_token: String,
    access_token: String,
    id_token: String,
}
