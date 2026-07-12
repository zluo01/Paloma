#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type ProviderMeta = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationItem {
    UserPrompt {
        prompt: String,
    },
    Message {
        message: Vec<MessageContentItem>,
        #[serde(default, skip_serializing_if = "ProviderMeta::is_empty")]
        provider_meta: ProviderMeta,
    },
    Reasoning {
        reasoning: Vec<SummaryItem>,
        #[serde(default, skip_serializing_if = "ProviderMeta::is_empty")]
        provider_meta: ProviderMeta,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
        #[serde(default, skip_serializing_if = "ProviderMeta::is_empty")]
        provider_meta: ProviderMeta,
    },
    ToolResult {
        call_id: String,
        name: String,
        output: String,
    },
    HostedTool {
        function_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "ProviderMeta::is_empty")]
        provider_meta: ProviderMeta,
    },
    // placeholder for model specific message
    Unknown {
        provider_meta: ProviderMeta,
    },
}

impl ConversationItem {
    pub fn payload_type(&self) -> &'static str {
        match self {
            ConversationItem::UserPrompt { .. } => "user_prompt",
            ConversationItem::Message { .. } => "message",
            ConversationItem::Reasoning { .. } => "reasoning",
            ConversationItem::ToolCall { .. } => "tool_call",
            ConversationItem::ToolResult { .. } => "tool_result",
            ConversationItem::HostedTool { .. } => "hosted_tool",
            ConversationItem::Unknown { .. } => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageContentItem {
    pub content: String,
    #[serde(default, skip_serializing_if = "ProviderMeta::is_empty")]
    pub provider_meta: ProviderMeta,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SummaryItem {
    pub content: String,
    pub provider_meta: ProviderMeta,
}

pub enum EncodeMode {
    SameProviderReplay,
    CrossProvider,
}
