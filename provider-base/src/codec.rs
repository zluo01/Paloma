use std::collections::{BTreeMap, HashMap};

use paloma_provider_protocol::v1::{
    ConversationItem, EncodeMode, MessageContentItem, SummaryItem, conversation_item::Item,
};
use serde_json::Value;

use super::Result;

pub type ProviderMeta = HashMap<String, String>;

pub trait ProviderEncoder: Send + Sync {
    fn encode_conversation_item(
        &self,
        item: &ConversationItem,
        encode_mode: EncodeMode,
    ) -> Option<Value> {
        match item.item.as_ref()? {
            Item::UserPrompt(prompt) => Some(self.encode_user_prompt(&prompt.prompt)),
            Item::Message(message) => {
                Some(self.encode_message(&message.message, &message.provider_meta, encode_mode))
            },
            Item::Reasoning(reasoning) => {
                self.encode_reasoning(&reasoning.reasoning, &reasoning.provider_meta, encode_mode)
            },
            Item::ToolCall(call) => Some(self.encode_tool_call(
                &call.call_id,
                &call.name,
                &call.arguments,
                &call.provider_meta,
                encode_mode,
            )),
            Item::ToolResult(result) => {
                Some(self.encode_tool_call_result(&result.call_id, &result.name, &result.output))
            },
            Item::HostedTool(tool) => self.encode_hosted_tool(
                &tool.function_type,
                &tool.content,
                &tool.provider_meta,
                encode_mode,
            ),
            Item::Unknown(unknown) => self.encode_unknown(&unknown.provider_meta, encode_mode),
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

/// Collect every field of `item` except `normalized_fields` into wire
/// metadata. Each entry value is one serde_json::Value serialized as JSON
/// text, per the `ProviderMeta` contract.
pub fn provider_meta(item: &Value, normalized_fields: &[&str]) -> ProviderMeta {
    item.as_object()
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| !normalized_fields.contains(&key.as_str()))
                .filter_map(|(key, value)| {
                    serde_json::to_string(value)
                        .ok()
                        .map(|value| (key.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn provider_meta_to_map(
    provider_meta: &ProviderMeta,
    include_provider_meta: bool,
) -> serde_json::Map<String, Value> {
    if include_provider_meta {
        provider_meta
            .iter()
            .filter_map(|(key, value)| {
                serde_json::from_str(value)
                    .ok()
                    .map(|value| (key.clone(), value))
            })
            .collect()
    } else {
        serde_json::Map::new()
    }
}
