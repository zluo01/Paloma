use crate::{ProviderId, capability::Item, permission::UserDecision};

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
    /// handler unique name
    pub id: &'static str,
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
        provider_id: ProviderId,
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
