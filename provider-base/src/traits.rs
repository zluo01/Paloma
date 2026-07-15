use scry_provider_protocol::v1::{
    ChatRequest, ConnectionPayload, Model, ProviderAuth, ProviderHealthStatus,
    finalize_connection_request, request_event,
};

use crate::{dispatcher::Dispatcher, error::Result};

#[async_trait::async_trait]
pub trait ProviderService: Send + Sync + 'static {
    async fn handle(
        &self,
        backend_id: Option<String>,
        payload: request_event::Payload,
        dispatcher: Dispatcher,
    );
}

#[async_trait::async_trait]
pub trait ProviderClient: Send + Sync {
    fn id(&self) -> String;

    async fn chat(&self, request: ChatRequest, dispatcher: Dispatcher) -> Result<()>;

    async fn models(&self) -> Option<Vec<Model>>;

    fn health_status(&self) -> ProviderHealthStatus;

    fn error(&self) -> Option<String>;
}

#[async_trait::async_trait]
pub trait ProviderAuthenticator: Send + Sync {
    fn id(&self) -> String;

    async fn init_connection(&self) -> Result<ConnectionPayload>;

    async fn finalize_connection(
        &self,
        input: finalize_connection_request::Input,
    ) -> Result<ProviderAuth>;
}
