use scry_extension_protocol::v1::Item;

use crate::{
    entity::{ExtensionCapabilityId, ProviderBackendId},
    permission::UserDecision,
};

#[derive(Clone, Debug)]
pub enum RenderEvent {
    Search(SearchRenderEvent),
    Chat(ChatRenderEvent),
    Cancel,
    Done,
    Error { message: String },
}

#[derive(Clone, Debug)]
pub enum SearchRenderEvent {
    Append { response: QueryResponse },
}

#[derive(Clone, Debug)]
pub struct QueryResponse {
    pub extension_capability_id: ExtensionCapabilityId,
    /// Display section name
    pub name: String,
    /// handler results
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum ChatRenderEvent {
    UserPrompt {
        text: String,
    },
    TextDelta {
        provider_backend_id: ProviderBackendId,
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCall {
        name: String,
        arguments: String,
        description: Option<String>,
        decisions: Vec<UserDecision>,
    },
}
