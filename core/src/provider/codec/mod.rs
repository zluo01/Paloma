mod claude;
mod codex;
mod schema;

use std::collections::BTreeMap;

pub use claude::ClaudeCodec;
pub use codex::CodexCodec;
pub use schema::{ConversationItem, EncodeMode, MessageContentItem, ProviderMeta};
use serde_json::Value;

use super::Result;
use crate::provider::codec::schema::SummaryItem;

pub trait ProviderEncoder: Send + Sync {
    fn encode_conversation_item(
        &self,
        item: &ConversationItem,
        encode_mode: EncodeMode,
    ) -> Option<Value> {
        match item {
            ConversationItem::UserPrompt { prompt } => Some(self.encode_user_prompt(prompt)),
            ConversationItem::Message {
                message,
                provider_meta,
            } => Some(self.encode_message(message, provider_meta, encode_mode)),
            ConversationItem::ToolCall {
                call_id,
                name,
                arguments,
                provider_meta,
            } => Some(self.encode_tool_call(call_id, name, arguments, provider_meta, encode_mode)),
            ConversationItem::ToolResult {
                call_id,
                name,
                output,
            } => Some(self.encode_tool_call_result(call_id, name, output)),
            ConversationItem::Reasoning {
                reasoning,
                provider_meta,
            } => self.encode_reasoning(reasoning, provider_meta, encode_mode),
            ConversationItem::HostedTool {
                function_type,
                content,
                provider_meta,
            } => self.encode_hosted_tool(function_type, content, provider_meta, encode_mode),
            ConversationItem::Unknown { provider_meta } => {
                self.encode_unknown(provider_meta, encode_mode)
            },
        }
    }

    fn encode_env_context(&self, envs: &BTreeMap<&'static str, String>) -> Value;

    fn encode_user_prompt(&self, prompt: &str) -> Value;

    fn encode_message(
        &self,
        message: &[MessageContentItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value;

    fn encode_reasoning(
        &self,
        content: &[SummaryItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value>;

    fn encode_tool_call(
        &self,
        call_id: &str,
        name: &str,
        arguments: &str,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value;

    fn encode_tool_call_result(&self, call_id: &str, name: &str, tool_output: &str) -> Value;

    fn encode_hosted_tool(
        &self,
        function_type: &str,
        content: &Option<String>,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value>;

    fn encode_unknown(
        &self,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value>;
}

pub trait ProviderDecoder: Send + Sync {
    fn decode_output_text_delta(&self, data: Value) -> Result<String>;

    fn decode_reasoning_delta(&self, data: Value) -> Result<String>;

    fn decode_output_item(&self, data: Value) -> Result<ConversationItem>;
}

fn provider_meta(item: &Value, normalized_fields: &[&str]) -> ProviderMeta {
    item.as_object()
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| !normalized_fields.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn provider_meta_to_map(
    provider_meta: &ProviderMeta,
    include_provider_meta: bool,
) -> serde_json::Map<String, Value> {
    if include_provider_meta {
        provider_meta
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    } else {
        serde_json::Map::new()
    }
}
