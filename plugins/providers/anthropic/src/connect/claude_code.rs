use std::sync::Mutex;

use oauth2::PkceCodeChallenge;
use scry_provider_base::{ProviderAuthenticator, ProviderError, Result};
use scry_provider_protocol::v1::{
    BrowserRedirect, ConnectionPayload, ProviderAuth, connection_payload,
    finalize_connection_request, provider_auth,
};
use serde::{Deserialize, Serialize};

use crate::constant::backend_id;

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTH_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
const SCOPES: &[&str] = &[
    "org:create_api_key",
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
];

pub struct ClaudeCodeConnector {
    request: reqwest::Client,
    pending: Mutex<Option<PendingOAuth>>,
}

impl ClaudeCodeConnector {
    pub fn new(request: reqwest::Client) -> Self {
        Self {
            request,
            pending: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl ProviderAuthenticator for ClaudeCodeConnector {
    fn id(&self) -> String {
        backend_id::CLAUDE_CODE.into()
    }

    async fn init_connection(&self) -> Result<ConnectionPayload> {
        let (code_challenge, code_verifier) = PkceCodeChallenge::new_random_sha256();
        let code_verifier = code_verifier.secret().to_string();
        let mut url = reqwest::Url::parse(AUTH_URL).map_err(|error| {
            ProviderError::Other(format!("invalid Claude Code auth URL: {error}"))
        })?;

        url.query_pairs_mut()
            .append_pair("client_id", CLIENT_ID)
            .append_pair("response_type", "code")
            .append_pair("scope", &SCOPES.join(" "))
            .append_pair("code_challenge", code_challenge.as_str())
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &code_verifier)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("code", "true");

        *self.pending.lock().unwrap() = Some(PendingOAuth { code_verifier });

        Ok(ConnectionPayload {
            payload: Some(connection_payload::Payload::BrowserRedirect(
                BrowserRedirect {
                    authorization_url: url.to_string(),
                },
            )),
        })
    }

    async fn finalize_connection(
        &self,
        input: finalize_connection_request::Input,
    ) -> Result<ProviderAuth> {
        let finalize_connection_request::Input::AuthorizationResponse(authorization_response) =
            input
        else {
            return Err(ProviderError::InvalidConnection {
                expected: "AuthorizationResponse",
            });
        };
        let pending = self.pending.lock().unwrap().clone().ok_or_else(|| {
            ProviderError::Other("Claude Code OAuth flow was not initialized".into())
        })?;
        let (code, state) = parse_authorization_response(&authorization_response)?;

        let refresh_token = self
            .request
            .post(TOKEN_URL)
            .header("Content-Type", "application/json")
            .json(&OAuthTokenRequest {
                code,
                state: state.unwrap_or_else(|| pending.code_verifier.clone()),
                grant_type: "authorization_code",
                client_id: CLIENT_ID,
                redirect_uri: REDIRECT_URI,
                code_verifier: &pending.code_verifier,
            })
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?
            .refresh_token
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::Other(
                    "Claude Code OAuth response did not include a refresh_token".into(),
                )
            })?;

        *self.pending.lock().unwrap() = None;

        Ok(ProviderAuth {
            payload: Some(provider_auth::Payload::RefreshToken(refresh_token)),
        })
    }
}

#[derive(Clone)]
struct PendingOAuth {
    code_verifier: String,
}

#[derive(Serialize)]
struct OAuthTokenRequest<'a> {
    code: String,
    state: String,
    grant_type: &'a str,
    client_id: &'a str,
    redirect_uri: &'a str,
    code_verifier: &'a str,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    refresh_token: Option<String>,
}

fn parse_authorization_response(input: &str) -> Result<(String, Option<String>)> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ProviderError::Other(
            "Claude Code authorization response is required".into(),
        ));
    }

    if let Ok(url) = reqwest::Url::parse(input) {
        let code = url
            .query_pairs()
            .find_map(|(key, value)| (key == "code").then(|| value.into_owned()));
        if let Some(code) = code {
            let state = url
                .query_pairs()
                .find_map(|(key, value)| (key == "state").then(|| value.into_owned()));
            return Ok((code, state));
        }
    }

    if let Some((code, state)) = input.split_once('#') {
        return Ok((code.to_string(), Some(state.to_string())));
    }

    Ok((input.to_string(), None))
}
