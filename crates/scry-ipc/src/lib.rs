use serde::{Deserialize, Serialize};
use tokio_util::codec::LengthDelimitedCodec;

pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024; // 8 mb

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum WireMessage {
    // daemon to UI
    #[serde(rename = "ui.toggle")]
    UiToggle,
    #[serde(rename = "result.begin")]
    ResultBegin(ResultBeginParams),
    #[serde(rename = "result.items")]
    ResultItems(ResultItemsParams),
    /// Token-by-token streamed text for the LLM-backed answer panel.
    /// No UI consumer wired up yet; reserved so the wire contract is
    /// stable for the eventual streaming-text capability.
    #[serde(rename = "result.append")]
    ResultAppend(ResultAppendParams),
    #[serde(rename = "result.end")]
    ResultEnd(ResultEndParams),
    #[serde(rename = "result.error")]
    ResultError(ResultErrorParams),
    #[serde(rename = "action.outcome")]
    ActionOutcome(ActionOutcomeParams),

    // UI to daemon
    #[serde(rename = "launcher.query")]
    LauncherQuery(LauncherQueryParams),
    #[serde(rename = "action.invoke")]
    ActionInvoke(ActionInvokeParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherQueryParams {
    pub query_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultBeginParams {
    pub query_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultAppendParams {
    pub query_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultEndParams {
    pub query_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultErrorParams {
    pub query_id: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultItemsParams {
    pub query_id: u64,
    pub handler_id: String,
    pub handler_name: String,
    pub items: Vec<scry_capability::Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInvokeParams {
    pub handler_id: String,
    pub action: scry_capability::Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcomeParams {
    pub outcome: scry_capability::ActionOutcome,
}

pub fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .length_field_type::<u32>()
        .max_frame_length(MAX_FRAME_BYTES)
        .new_codec()
}
