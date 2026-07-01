use crate::{
    Connection, ProviderId,
    provider::{Auth, ProviderAuthenticator, ProviderError, Result},
};

pub struct ApiKeyConnector {
    provider_id: ProviderId,
    instructions_url: &'static str,
}

impl ApiKeyConnector {
    pub const fn openai() -> Self {
        Self {
            provider_id: ProviderId::OpenAI,
            instructions_url: "https://platform.openai.com/api-keys",
        }
    }

    pub const fn anthropic() -> Self {
        Self {
            provider_id: ProviderId::Anthropic,
            instructions_url: "https://console.anthropic.com/settings/keys",
        }
    }
}

#[async_trait::async_trait]
impl ProviderAuthenticator for ApiKeyConnector {
    fn id(&self) -> ProviderId {
        self.provider_id
    }

    async fn init_connection(&self) -> Result<Connection> {
        Ok(Connection::ManualInput {
            api_key: String::new(),
            instructions_url: Some(self.instructions_url.to_string()),
        })
    }

    async fn finalize_connection(&self, payload: Connection) -> Result<Auth> {
        let Connection::ManualInput { api_key, .. } = payload else {
            return Err(ProviderError::InvalidConnection {
                expected: "ManualInput",
            });
        };

        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ProviderError::Other(format!(
                "{} API key is required",
                self.provider_id
            )));
        }

        Ok(Auth::ApiKey(api_key.to_string()))
    }
}
