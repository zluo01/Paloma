use std::time::Duration;

use paloma_provider_protocol::v1::{ProviderAuth, provider_auth::Payload};
use serde::Serialize;

use crate::error::{ProviderError, Result};

const REFRESH_TIMEOUT_BUFFER: u64 = Duration::from_mins(5).as_secs();

#[derive(Clone)]
pub struct OAuthState {
    pub refresh_token: String,
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_at: Option<u64>,
}

impl OAuthState {
    pub fn new(refresh_token: String) -> Self {
        Self {
            refresh_token,
            expires_at: None,
        }
    }

    pub fn rotated(refresh_token: String, expires_at: u64) -> Self {
        Self {
            refresh_token,
            expires_at: Some(expires_at),
        }
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub fn is_fresh(&self, now: u64) -> bool {
        self.expires_at
            .is_some_and(|expires_at| now <= expires_at.saturating_sub(REFRESH_TIMEOUT_BUFFER))
    }
}

impl TryFrom<&ProviderAuth> for OAuthState {
    type Error = ProviderError;

    fn try_from(auth: &ProviderAuth) -> Result<Self> {
        match auth.payload.as_ref() {
            Some(Payload::RefreshToken(refresh_token)) => Ok(Self::new(refresh_token.clone())),
            Some(Payload::ApiKey(_)) => Err(ProviderError::Other(
                "api_key credential is not supported by an oauth backend".into(),
            )),
            None => Err(ProviderError::Other("missing a refresh_token".into())),
        }
    }
}

#[derive(Serialize)]
pub struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

impl<'a> RefreshRequest<'a> {
    /// Build the refresh-grant body for the current OAuth credential.
    pub fn new(state: &'a OAuthState, client_id: &'a str) -> Self {
        Self {
            client_id,
            grant_type: "refresh_token",
            refresh_token: state.refresh_token(),
        }
    }
}
