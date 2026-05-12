use crate::entity::{ChatRequest, ChatStream, Model, ProviderClient, ProviderError, ProviderId};
use crate::{Auth, Result};
use base64::Engine;
use scry_storage::storage::Storage;
use serde::Deserialize;
use std::sync::{Arc, RwLock};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
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
const CLIENT_VERSION: &str = "0.130.0";

pub struct CodexRuntime {
    request: reqwest::Client,
    refresh_token: Arc<RwLock<String>>,
    tokens: Arc<RwLock<RefreshTokens>>,
}

impl CodexRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client) -> Result<Self> {
        let refresh_token = match credential {
            Auth::OAuth {
                refresh_token: Some(rt),
                ..
            } => rt.clone(),
            Auth::OAuth {
                refresh_token: None,
                ..
            } => {
                return Err(ProviderError::Other(
                    "Codex credential is missing a refresh_token".into(),
                ));
            }
            Auth::ApiKey(_) => {
                return Err(ProviderError::Other(
                    "Codex does not support api_key credentials".into(),
                ));
            }
        };

        Ok(Self {
            request,
            refresh_token: Arc::new(RwLock::new(refresh_token)),
            tokens: Arc::new(RwLock::new(RefreshTokens {
                access_token: String::new(),
                chatgpt_account_id: String::new(),
            })),
        })
    }
}

async fn fetch_access_token(
    request: &reqwest::Client,
    refresh_token: &str,
) -> Result<(RefreshTokens, String, i64)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let chatgpt_account_id = extract_chatgpt_account_id(&response.id_token)
        .ok_or_else(|| ProviderError::Other("id_token missing chatgpt_account_id claim".into()))?;

    Ok((
        RefreshTokens {
            access_token: response.access_token,
            chatgpt_account_id,
        },
        response.refresh_token,
        response.expires_in,
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

#[async_trait::async_trait]
impl ProviderClient for CodexRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn refresh(&self, storage: &Storage) -> Result<Option<Auth>> {
        let current_refresh_token = self.refresh_token.read().unwrap().clone();
        let (new_tokens, new_refresh_token, expires_in) =
            fetch_access_token(&self.request, &current_refresh_token).await?;
        *self.tokens.write().unwrap() = new_tokens;
        *self.refresh_token.write().unwrap() = new_refresh_token.clone();

        // codex refresh token follows "rotate on use"
        // so we need to proactively update the db whenever we refresh.
        storage
            .update_provider(ProviderId::Codex.as_str(), "oauth", &new_refresh_token)
            .await?;

        Ok(Some(Auth::OAuth {
            refresh_token: Some(new_refresh_token),
            expires_in: Some(expires_in),
        }))
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatStream> {
        todo!()
    }

    async fn models(&self) -> Result<Vec<Model>> {
        let (access_token, account_id) = {
            let guard = self.tokens.read().unwrap();
            (guard.access_token.clone(), guard.chatgpt_account_id.clone())
        };

        let url = format!("{MODELS_URL}?client_version={CLIENT_VERSION}");
        let response: ModelsResponse = self
            .request
            .get(&url)
            .bearer_auth(&access_token)
            .header("chatgpt-account-id", &account_id)
            .header("OpenAI-Beta", "responses=experimental")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut models: Vec<RawModel> = response
            .models
            .into_iter()
            .filter(|m| m.supported_in_api && m.visibility == "list")
            .collect();
        // Higher priority first; tie-break alphabetically on slug.
        models.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.slug.cmp(&b.slug))
        });

        Ok(models
            .into_iter()
            .map(|m| Model {
                id: m.slug,
                name: m.display_name,
                default_reasoning_effort: m.default_reasoning_level.unwrap_or("medium".to_string()),
                supported_reasoning_efforts: m
                    .supported_reasoning_levels
                    .into_iter()
                    .map(|p| p.effort)
                    .collect(),
            })
            .collect())
    }
}

/// Tokens derived from a refresh-token exchange. Held together under one lock
/// so a refresh rotates `access_token` (and re-derives `chatgpt_account_id`)
/// atomically.
struct RefreshTokens {
    access_token: String,
    chatgpt_account_id: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_in: i64,
    refresh_token: String,
    access_token: String,
    id_token: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    models: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    slug: String,
    display_name: String,
    visibility: String,
    supported_in_api: bool,
    priority: i32,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<RawReasoningEffortPreset>,
}

#[derive(Debug, Deserialize)]
struct RawReasoningEffortPreset {
    effort: String,
}
