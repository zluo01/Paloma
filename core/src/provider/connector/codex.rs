use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    entity::ProviderId,
    provider::{Auth, Connection, ProviderAuthenticator, ProviderError, Result},
};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const TOKEN_POLL_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const TOKEN_EXCHANGE_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const VERIFICATION_URI: &str = "https://auth.openai.com/codex/device";

/// Defensive ceiling on how long we wait for the user to enter the device
/// code. The real deadline comes from the server's `expires_at`; this caps
/// the wait in case the server returns something unreasonable.
const DEVICE_AUTH_TIMEOUT: Duration = Duration::from_secs(900);
/// Added on top of the server's suggested interval to dodge rate limits.
const POLL_SAFETY_MARGIN: Duration = Duration::from_secs(3);

pub struct CodexConnector {
    request: reqwest::Client,
}

impl CodexConnector {
    pub fn new(request: reqwest::Client) -> Self {
        Self { request }
    }

    /// Poll the device-auth token endpoint until the user enters the code
    /// (HTTP 200) or the timeout fires.
    async fn poll_for_device_token(
        &self,
        device_auth_id: &str,
        user_code: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<DeviceTokenResponse> {
        let start = tokio::time::Instant::now();
        loop {
            if start.elapsed() >= timeout {
                return Err(ProviderError::Timeout(timeout.as_secs()));
            }
            tokio::time::sleep(poll_interval).await;

            let response = self
                .request
                .post(TOKEN_POLL_URL)
                .json(&serde_json::json!({
                    "device_auth_id": device_auth_id,
                    "user_code": user_code,
                }))
                .send()
                .await?;

            let status = response.status();
            if status.is_success() {
                return Ok(response.json::<DeviceTokenResponse>().await?);
            }
            // 403/404 = authorization pending. Anything else is terminal.
            if status.as_u16() == 403 || status.as_u16() == 404 {
                continue;
            }
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::PollFailed {
                status: status.as_u16(),
                body,
            });
        }
    }
}

#[async_trait::async_trait]
impl ProviderAuthenticator for CodexConnector {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn init_connection(&self) -> Result<Connection> {
        let resp: UserCodeResponse = self
            .request
            .post(USERCODE_URL)
            .json(&serde_json::json!({ "client_id": CLIENT_ID }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(Connection::DeviceCode {
            verification_uri: VERIFICATION_URI,
            user_code: resp.user_code.clone(),
            transaction_payload: serde_json::to_value(&resp)?,
        })
    }

    async fn finalize_connection(&self, payload: Connection) -> Result<Auth> {
        let transactional_payload = match payload {
            Connection::DeviceCode {
                transaction_payload,
                ..
            } => transaction_payload,
            _ => {
                return Err(ProviderError::InvalidConnection {
                    expected: "DeviceCode",
                });
            },
        };
        let transaction_payload: UserCodeResponse = serde_json::from_value(transactional_payload)?;

        let interval_secs = transaction_payload
            .interval
            .parse::<u64>()
            .unwrap_or(5)
            .max(1);
        let poll_interval = Duration::from_secs(interval_secs) + POLL_SAFETY_MARGIN;

        // The server-side authorization window expires at `expires_at`.
        // Past it, every poll returns an error, so the effective timeout is
        // the smaller of the server's remaining window and our defensive cap.
        let server_deadline = chrono::DateTime::parse_from_rfc3339(&transaction_payload.expires_at)
            .map_err(|source| ProviderError::ParseTimestamp {
                field: "expires_at",
                source,
            })?;
        let server_remaining = (server_deadline.with_timezone(&chrono::Utc) - chrono::Utc::now())
            .to_std()
            .unwrap_or(Duration::ZERO);
        let timeout = server_remaining.min(DEVICE_AUTH_TIMEOUT);

        let device_token = self
            .poll_for_device_token(
                &transaction_payload.device_auth_id,
                &transaction_payload.user_code,
                poll_interval,
                timeout,
            )
            .await?;

        let tokens: OAuthTokenResponse = self
            .request
            .post(TOKEN_EXCHANGE_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", device_token.authorization_code.as_str()),
                ("redirect_uri", REDIRECT_URI),
                ("client_id", CLIENT_ID),
                ("code_verifier", device_token.code_verifier.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(Auth::OAuth {
            refresh_token: tokens.refresh_token,
            expires_at: None,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UserCodeResponse {
    device_auth_id: String,
    user_code: String,
    interval: String,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    refresh_token: Option<String>,
}
