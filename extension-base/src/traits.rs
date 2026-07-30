use async_trait::async_trait;
use paloma_extension_protocol::v1::{
    Action, Item, ToolContent, ToolFacet, run_action_response::Behavior,
};

pub trait Capability: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;

    fn search_handler(&self) -> Option<&dyn SearchHandler> {
        None
    }

    fn tool_handler(&self) -> Option<&dyn ToolHandler> {
        None
    }
}

pub trait SearchHandler {
    fn search(&self, input: &str) -> Vec<Item>;

    fn run_search_action(&self, action: Action) -> Behavior;
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn facet(&self) -> ToolFacet;

    async fn invoke(
        &self,
        session_id: &str,
        call_id: &str,
        arguments: &str,
    ) -> Result<ToolContent, String>;

    async fn cancel(&self, session_id: &str) -> Result<(), String>;
}
