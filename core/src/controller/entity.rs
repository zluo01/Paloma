use crate::{capability::Item, controller::UserDecision};

#[derive(Clone, Debug)]
pub enum RenderEvent {
    Local(LocalRenderEvent),
    Chat(ChatRenderEvent),
    Cancel,
    Done,
    Error { message: String },
}

#[derive(Clone, Debug)]
pub enum LocalRenderEvent {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthLevel {
    /// Nothing active (grey).
    Inactive,
    /// Everything running (green).
    Healthy,
    /// Something is wrong (orange).
    Degraded,
    /// Everything down (red).
    Down,
}

impl HealthLevel {
    pub fn from_counts(total: usize, healthy: usize) -> Self {
        match (total, healthy) {
            (0, _) => HealthLevel::Inactive,
            (total, healthy) if healthy == total => HealthLevel::Healthy,
            (_, 0) => HealthLevel::Down,
            _ => HealthLevel::Degraded,
        }
    }
}
