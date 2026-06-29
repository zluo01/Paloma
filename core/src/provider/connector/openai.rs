use crate::{
    Connection, ProviderId,
    provider::{Auth, ProviderAuthenticator, ProviderError, Result},
};

const API_KEYS_URL: &str = "https://platform.openai.com/api-keys";

pub struct OpenAIConnector;

impl OpenAIConnector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProviderAuthenticator for OpenAIConnector {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAI
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
            return Err(ProviderError::Other("OpenAI API key is required".into()));
        }

        Ok(Auth::ApiKey(api_key.to_string()))
    }
}
