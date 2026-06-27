mod assistant;
mod reasoning;
mod tool_call;
mod user_prompt;

pub(super) use self::{
    assistant::AssistantSection,
    reasoning::ReasoningSection,
    tool_call::{ToolCallDecision, ToolCallSection},
    user_prompt::UserPromptSection,
};

pub(crate) enum Section {
    UserPrompt(()),
    Reasoning(ReasoningSection),
    Assistant(AssistantSection),
    ToolCall(()),
}

impl Section {
    pub(crate) fn complete(&self) {
        if let Section::Assistant(assistant) = self {
            assistant.complete();
        }
    }
}
