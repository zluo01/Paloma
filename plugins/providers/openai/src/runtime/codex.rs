use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use base64::Engine;
use log::error;
use scry_provider_base::{
    Dispatcher, OAuthState, ProviderCache, ProviderClient, ProviderError, RefreshRequest, Result,
    cached_models,
};
use scry_provider_protocol::v1::{
    AuthUpdateRequest, ChatRequest, Model, ProviderAuth, ProviderHealthStatus, response_event,
};
use scry_utils::{attempt_with_retry, unix_now};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::shared::{
    MODELS_FETCH_TIMEOUT, ModelsResponse, build_request_body, models_from_response,
    parse_stream_error, response_event_stream,
};
use crate::constant::backend_id;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CLIENT_VERSION_URL: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const CLIENT_VERSION_CACHE_KEY: &str = "codex.client_version";
const CLIENT_VERSION_CACHE_TTL_SECS: u64 = Duration::from_hours(24).as_secs();
const CLIENT_VERSION_FETCH_TIMEOUT: Duration = Duration::from_secs(3);
const CLIENT_VERSION_FALLBACK: &str = "0.144.0";

pub struct CodexRuntime {
    request: reqwest::Client,
    provider_cache: Arc<ProviderCache>,
    token_state: RwLock<Option<TokenState>>,
    refresh_lock: Mutex<()>,
    status: AtomicI32,
    error: RwLock<Option<String>>,
    dispatcher: Dispatcher,
}

impl CodexRuntime {
    pub async fn new(
        credential: &ProviderAuth,
        request: reqwest::Client,
        provider_cache: Arc<ProviderCache>,
        dispatcher: Dispatcher,
    ) -> Self {
        let oauth = match OAuthState::try_from(credential) {
            Ok(oauth) => oauth,
            Err(e) => {
                return Self::unhealthy(request, provider_cache, dispatcher, format!("Codex: {e}"));
            },
        };

        match fetch_access_token(&request, &oauth, &dispatcher).await {
            Ok((access_token, oauth)) => {
                let warmup = provider_cache
                    .models(backend_id::CODEX.into(), || {
                        let cache = Arc::clone(&provider_cache);
                        let request = request.clone();
                        let access_token = access_token.clone();
                        async move {
                            let version = client_version(&cache, &request).await;
                            fetch_models(&request, &access_token, &version).await
                        }
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
                            format!("fail to connect to codex: {e}"),
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
                    format!("fail to connect to codex: {e}"),
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
    async fn refresh(&self) -> Result<AccessToken> {
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

    fn cached_access_token(&self) -> Option<AccessToken> {
        let state = self.token_state.read().unwrap();
        let state = state.as_ref()?;
        state
            .oauth
            .is_fresh(unix_now())
            .then(|| state.access_token.clone())
    }
}

#[async_trait::async_trait]
impl ProviderClient for CodexRuntime {
    fn id(&self) -> String {
        backend_id::CODEX.into()
    }

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()> {
        let token = self
            .refresh()
            .await
            .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

        let body = build_request_body(&request, backend_id::CODEX.into());

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&token.access_token)
            .header("chatgpt-account-id", &token.chatgpt_account_id)
            .header("originator", "scry")
            .header(reqwest::header::USER_AGENT, "scry-codex")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await?;
            return Err(parse_stream_error(&body));
        }

        response_event_stream(response, backend_id::CODEX.into(), dispatcher).await;
        Ok(())
    }

    async fn models(self: Arc<Self>) -> Option<Vec<Model>> {
        let cache = Arc::clone(&self.provider_cache);

        cached_models(&cache, backend_id::CODEX.into(), move || async move {
            let token = self
                .refresh()
                .await
                .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

            let version = client_version(&self.provider_cache, &self.request).await;
            fetch_models(&self.request, &token, &version).await
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

async fn fetch_access_token(
    request: &reqwest::Client,
    oauth: &OAuthState,
    dispatcher: &Dispatcher,
) -> Result<(AccessToken, OAuthState)> {
    // Deliberately no timeout or retry: the exchange rotates the refresh token
    // server-side, so aborting or replaying a possibly-executed request can
    // orphan the rotated token. Waiting out the client's read timeout is safer.
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
                        backend_id: backend_id::CODEX.into(),
                        refresh_token,
                    },
                ))
                .await;
        }
    });

    let chatgpt_account_id = extract_chatgpt_account_id(&response.id_token)
        .ok_or_else(|| ProviderError::Other("id_token missing chatgpt_account_id claim".into()))?;

    // get the epoch time current access token will expire
    let expires_at = unix_now() + response.expires_in;

    Ok((
        AccessToken {
            access_token: response.access_token,
            chatgpt_account_id,
        },
        OAuthState::rotated(response.refresh_token, expires_at),
    ))
}

/// Decode a JWT's middle segment (claims) and pull out
/// `https://api.openai.com/auth.chatgpt_account_id`.
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

async fn fetch_models(
    request: &reqwest::Client,
    token: &AccessToken,
    client_version: &str,
) -> Result<Vec<Model>> {
    let response: ModelsResponse = attempt_with_retry(
        || async move {
            Ok(request
                .get(format!("{MODELS_URL}?client_version={client_version}"))
                .bearer_auth(&token.access_token)
                .header("chatgpt-account-id", &token.chatgpt_account_id)
                .header("OpenAI-Beta", "responses=experimental")
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

/// Construction in Codex (major.minor.patch from `CARGO_PKG_VERSION`):
///   <https://github.com/openai/codex/blob/main/codex-rs/models-manager/src/lib.rs#L19-L26>
/// Query-param append:
///   <https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/models.rs#L35-L38>
async fn client_version(cache: &ProviderCache, request: &reqwest::Client) -> String {
    cache
        .value(
            CLIENT_VERSION_CACHE_KEY,
            CLIENT_VERSION_CACHE_TTL_SECS,
            || {
                let request = request.clone();
                async move { fetch_client_version(&request).await }
            },
        )
        .await
        .unwrap_or_else(|e| {
            error!(
                "fail to fetch codex client version, fallback to {CLIENT_VERSION_FALLBACK}. {e}"
            );
            CLIENT_VERSION_FALLBACK.into()
        })
}

async fn fetch_client_version(request: &reqwest::Client) -> Result<String> {
    let release: serde_json::Value = request
        .get(CLIENT_VERSION_URL)
        .header(reqwest::header::USER_AGENT, "scry")
        .timeout(CLIENT_VERSION_FETCH_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    release["name"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ProviderError::Other("latest release response is missing a name".into()))
}

/// OAuth and access-token data derived from a refresh-token exchange. Held
/// together so refresh-token rotation and access-token replacement are atomic
/// from readers' perspective.
#[derive(Clone)]
struct TokenState {
    oauth: OAuthState,
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
