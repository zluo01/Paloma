use crate::QueryResponse;

pub enum RenderEvent {
    Local(LocalRenderEvent),
    Chat(ChatRenderEvent),
    Done,
    Error { message: String },
}

pub enum LocalRenderEvent {
    Append { response: QueryResponse },
}

pub enum ChatRenderEvent {
    TextDelta { text: String },
    ReasoningDelta { text: String },
}
