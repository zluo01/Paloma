use paloma_provider_protocol::v1::{
    ConnectionPayload, ManualInput, ProviderAuth, connection_payload, finalize_connection_request,
    provider_auth,
};

use crate::{
    error::{ProviderError, Result},
    traits::ProviderAuthenticator,
};

pub struct ApiKeyConnector {
    pub backend_id: String,
    pub instructions_url: &'static str,
}

#[async_trait::async_trait]
impl ProviderAuthenticator for ApiKeyConnector {
    fn id(&self) -> String {
        self.backend_id.clone()
    }

    async fn init_connection(&self) -> Result<ConnectionPayload> {
        Ok(ConnectionPayload {
            payload: Some(connection_payload::Payload::ManualInput(ManualInput {
                api_key: String::new(),
                instructions_url: Some(self.instructions_url.to_string()),
            })),
        })
    }

    async fn finalize_connection(
        &self,
        input: finalize_connection_request::Input,
    ) -> Result<ProviderAuth> {
        let finalize_connection_request::Input::ApiKey(api_key) = input else {
            return Err(ProviderError::InvalidConnection { expected: "ApiKey" });
        };

        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ProviderError::Other(format!(
                "{} API key is required",
                self.backend_id
            )));
        }

        Ok(ProviderAuth {
            payload: Some(provider_auth::Payload::ApiKey(api_key.to_string())),
        })
    }
}
