use crate::{
    Connection, ProviderId,
    provider::{Auth, ProviderAuthenticator, ProviderError, Result},
};

const API_KEYS_URL: &str = "https://console.anthropic.com/settings/keys";

pub struct AnthropicConnector;

impl AnthropicConnector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProviderAuthenticator for AnthropicConnector {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    async fn init_connection(&self) -> Result<Connection> {
        Ok(Connection::ManualInput {
            api_key: "".to_string(),
            instructions_url: Some(API_KEYS_URL.to_string()),
        })
    }

    async fn finalize_connection(&self, payload: Connection) -> Result<Auth> {
        let api_key = match payload {
            Connection::ManualInput { api_key, .. } => api_key,
            _ => {
                return Err(ProviderError::InvalidConnection {
                    expected: "ManualInput",
                });
            },
        };

        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ProviderError::Other("Anthropic API key is required".into()));
        }

        Ok(Auth::ApiKey(api_key.to_string()))
    }
}
