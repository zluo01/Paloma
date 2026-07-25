use async_trait::async_trait;
use scry_extension_protocol::v1::{Action, Item, ToolFacet, run_action_response::Behavior};
use tokio_util::sync::CancellationToken;

pub trait Capability: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;

    fn search_handler(&self) -> Option<&dyn SearchHandler>;
    fn tool_handler(&self) -> Option<&dyn ToolHandler>;
}

pub trait SearchHandler {
    fn search(&self, input: &str) -> Vec<Item>;

    fn run_search_action(&self, action: Action) -> Behavior;
}

#[async_trait]
pub trait ToolHandler {
    fn facet(&self) -> ToolFacet;

    async fn invoke(
        &self,
        cancel: CancellationToken,
        call_id: &str,
        arguments: &str,
    ) -> Result<String, String>;
}
