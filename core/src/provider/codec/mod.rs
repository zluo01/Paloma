#![allow(dead_code)]

mod codex;
mod schema;

use std::collections::BTreeMap;

pub use codex::CodexCodec;
#[allow(unused_imports)]
pub use schema::{ConversationItem, MessageContentItem, ProviderMeta};
use serde_json::Value;

use super::Result;
use crate::provider::codec::schema::{EncodeMode, SummaryItem};

pub trait ProviderEncoder: Send + Sync {
    fn encode_env_context(&self, envs: BTreeMap<&'static str, String>) -> String;

    fn encode_user_prompt(&self, prompt: &str) -> Value;

    fn encode_message(
        &self,
        message: &[MessageContentItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value;

    fn encode_reasoning(
        &self,
        content: &Vec<SummaryItem>,
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
}

pub trait ProviderDecoder: Send + Sync {
    fn decode_output_text_delta(&self, data: &str) -> Result<String>;

    fn decode_reasoning_delta(&self, data: &str) -> Result<String>;

    fn decode_output_item(&self, data: &str) -> Result<ConversationItem>;
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
