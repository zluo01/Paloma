use std::collections::{BTreeMap, HashMap};

use log::warn;
use paloma_provider_base::{
    ProviderDecoder, ProviderEncoder, ProviderError, ProviderMeta, Result, provider_meta,
    provider_meta_to_map,
};
use paloma_provider_protocol::v1::{
    ConversationItem, ConversationMessage, EncodeMode, HostedTool, MessageContentItem, Reasoning,
    SummaryItem, ToolCall, Unknown, conversation_item,
};
use paloma_utils::Element;
use serde_json::Value;

pub struct ClaudeCodec;

impl ClaudeCodec {
    pub(crate) const SUPPORTED_RESPONSE_TYPES: &'static [&'static str] = &[
        "text",
        "thinking",
        "tool_use",
        "server_tool_use",
        "redacted_thinking",
        "web_search_tool_result",
    ];
}

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L864-L880
impl ProviderEncoder for ClaudeCodec {
    fn encode_env_context(&self, envs: &BTreeMap<&'static str, String>) -> Value {
        let env_instruction = envs
            .iter()
            .fold(
                Element::new("environment_context"),
                |element, (key, value)| element.child(Element::new(*key).plain_text(value)),
            )
            .to_string();

        serde_json::json!({
            "type": "text",
            "text": env_instruction
        })
    }

    fn encode_user_prompt(&self, prompt: &str) -> Value {
        serde_json::json!({
            "role": "user",
            "content": prompt
        })
    }

    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1591-L1602
    fn encode_message(
        &self,
        message: &[MessageContentItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value {
        let same_provider = matches!(encode_mode, EncodeMode::SameProvider);
        let mut item = provider_meta_to_map(provider_meta, same_provider);

        item.insert("type".to_string(), Value::String("text".to_string()));
        item.insert(
            "text".to_string(),
            Value::String(message.iter().map(|m| m.content.as_str()).collect()),
        );

        message_param("assistant", Value::Object(item))
    }

    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1752-L1758
    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1449-L1453
    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L2989-L3005
    fn encode_reasoning(
        &self,
        content: &[SummaryItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value> {
        if !matches!(encode_mode, EncodeMode::SameProvider) {
            return None;
        }

        let mut item = provider_meta_to_map(provider_meta, true);

        let content_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("thinking");

        item.insert("type".to_string(), Value::String(content_type.to_string()));
        item.insert(
            "thinking".to_string(),
            Value::String(content.iter().map(|m| m.content.as_str()).collect()),
        );

        Some(message_param("assistant", Value::Object(item)))
    }

    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L2299-L2317
    fn encode_tool_call(
        &self,
        call_id: &str,
        name: &str,
        arguments: &str,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value {
        let same_provider = matches!(encode_mode, EncodeMode::SameProvider);
        let mut item = provider_meta_to_map(provider_meta, same_provider);

        item.insert("type".to_string(), Value::String("tool_use".to_string()));
        item.insert("id".to_string(), Value::String(call_id.to_string()));
        item.insert("name".to_string(), Value::String(name.to_string()));
        let input = serde_json::from_str::<serde_json::Map<String, Value>>(arguments)
            .map(Value::Object)
            .unwrap_or_else(|_| {
                warn!("fail to serialize arguments to object {}", arguments);
                Value::Object(Default::default())
            });
        item.insert("input".to_string(), input);

        message_param("assistant", Value::Object(item))
    }

    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L2014-L2035
    fn encode_tool_call_result(&self, call_id: &str, _name: &str, tool_output: &str) -> Value {
        let text_content = serde_json::json!({
            "type": "text",
            "text": tool_output
        });
        let item = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": call_id,
            "content": [
                text_content
            ],
        });
        message_param("user", item)
    }

    /// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1541-L1566
    fn encode_hosted_tool(
        &self,
        function_type: &str,
        content: &Option<String>,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value> {
        if !matches!(encode_mode, EncodeMode::SameProvider) {
            return None;
        }
        let mut item = provider_meta_to_map(provider_meta, true);

        item.insert(
            "type".to_string(),
            Value::String("server_tool_use".to_string()),
        );
        item.insert("name".to_string(), Value::String(function_type.to_string()));
        if let Some(content) = content {
            let input = serde_json::from_str::<serde_json::Map<String, Value>>(content)
                .map(Value::Object)
                .unwrap_or_else(|_| {
                    warn!("fail to serialize arguments to object {}", content);
                    Value::Object(Default::default())
                });
            item.insert("input".to_string(), input);
        }
        Some(message_param("assistant", Value::Object(item)))
    }

    fn encode_unknown(
        &self,
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Option<Value> {
        if !matches!(encode_mode, EncodeMode::SameProvider) {
            return None;
        }
        let item = provider_meta_to_map(provider_meta, true);
        Some(message_param("assistant", Value::Object(item)))
    }
}

fn message_param(role: &'static str, content: Value) -> Value {
    serde_json::json!({
        "role": role,
        "content": [content],
    })
}

impl ProviderDecoder for ClaudeCodec {
    /// https://platform.claude.com/docs/en/build-with-claude/streaming#text-delta
    fn decode_output_text_delta(&self, data: Value) -> Result<String> {
        data.get("text")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| ProviderError::Other("missing text delta field".into()))
    }

    /// https://platform.claude.com/docs/en/build-with-claude/streaming#thinking-delta
    fn decode_reasoning_delta(&self, data: Value) -> Result<String> {
        data.get("thinking")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| ProviderError::Other("missing reasoning delta field".into()))
    }

    fn decode_output_item(&self, data: Value) -> Result<ConversationItem> {
        let response_type = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Other("missing output item type".into()))?;

        match response_type {
            "text" => decode_message_item(&data),
            "thinking" => decode_reasoning_item(&data),
            "redacted_thinking" | "web_search_tool_result" => {
                let provider_meta = provider_meta(&data, &[]);
                Ok(ConversationItem {
                    item: Some(conversation_item::Item::Unknown(Unknown { provider_meta })),
                })
            },
            "tool_use" => decode_function_call_item(&data),
            "server_tool_use" => decode_hosted_tool_item(&data),
            _ => {
                warn!("unknown response type {}", response_type);
                Err(ProviderError::Other(format!(
                    "unknown response type {}",
                    response_type
                )))
            },
        }
    }
}

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1576-L1589
fn decode_message_item(data: &Value) -> Result<ConversationItem> {
    let message = data
        .get("text")
        .and_then(Value::as_str)
        .map(|t| MessageContentItem {
            content: t.to_string(),
            provider_meta: HashMap::new(),
        })
        .ok_or_else(|| ProviderError::Other("missing text field".into()))?;

    let provider_meta = provider_meta(data, &["type", "text"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::Message(ConversationMessage {
            message: vec![message],
            provider_meta,
        })),
    })
}

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1744-L1750
fn decode_reasoning_item(data: &Value) -> Result<ConversationItem> {
    let reasoning = data
        .get("thinking")
        .and_then(Value::as_str)
        .map(|t| SummaryItem {
            content: t.to_string(),
            provider_meta: HashMap::new(),
        })
        .ok_or_else(|| ProviderError::Other("missing thinking field".into()))?;

    let provider_meta = provider_meta(data, &["type", "thinking"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::Reasoning(Reasoning {
            reasoning: vec![reasoning],
            provider_meta,
        })),
    })
}

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L2284-L2297
fn decode_function_call_item(item: &Value) -> Result<ConversationItem> {
    let call_id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call id".into()))?
        .to_string();
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call name".into()))?
        .to_string();
    let arguments = decode_input_arguments(item)?;
    let provider_meta = provider_meta(item, &["type", "id", "name", "input"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::ToolCall(ToolCall {
            call_id,
            name,
            arguments,
            provider_meta,
        })),
    })
}

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1519-L1539
fn decode_hosted_tool_item(item: &Value) -> Result<ConversationItem> {
    let function_type = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call name".into()))?
        .to_string();
    let arguments = decode_input_arguments(item)?;
    let provider_meta = provider_meta(item, &["type", "name", "input"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::HostedTool(HostedTool {
            function_type,
            content: Some(arguments),
            provider_meta,
        })),
    })
}

/// properly handle input with zero arguments
fn decode_input_arguments(item: &Value) -> Result<String> {
    match item.get("input") {
        Some(Value::String(input)) => Ok(input.clone()),
        Some(input @ Value::Object(_)) => Ok(input.to_string()),
        _ => Err(ProviderError::Other(
            "missing function call arguments".into(),
        )),
    }
}

#[cfg(test)]
mod encoder_tests {
    use super::*;

    #[test]
    fn encodes_env_context_as_system_text_blocks() {
        let item = ClaudeCodec.encode_env_context(&BTreeMap::from([
            ("os", "linux".to_string()),
            ("os_family", "unix".to_string()),
            ("arch", "x86_64".to_string()),
            ("home", "/home/example".to_string()),
            ("shell", "/bin/bash".to_string()),
        ]));

        assert_eq!(
            item,
            serde_json::json!({
                "type": "text",
                "text": "<environment_context>\n<arch>x86_64</arch>\n<home>/home/example</home>\n<os>linux</os>\n<os_family>unix</os_family>\n<shell>/bin/bash</shell>\n</environment_context>"
            })
        );
    }

    #[test]
    fn encodes_user_prompt_as_user_message() {
        let item = ClaudeCodec.encode_user_prompt("Hello");

        assert_eq!(
            item,
            serde_json::json!({
                "role": "user",
                "content": "Hello"
            })
        );
    }

    #[test]
    fn encodes_message_as_assistant_message() {
        let item = ClaudeCodec.encode_message(
            &[
                MessageContentItem {
                    content: "Hello".to_string(),
                    provider_meta: HashMap::default(),
                },
                MessageContentItem {
                    content: " world".to_string(),
                    provider_meta: HashMap::default(),
                },
            ],
            &ProviderMeta::default(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Hello world"
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_reasoning_as_assistant_message() {
        let item = ClaudeCodec
            .encode_reasoning(
                &[SummaryItem {
                    content: "I need to think".to_string(),
                    provider_meta: HashMap::default(),
                }],
                &ProviderMeta::default(),
                EncodeMode::SameProvider,
            )
            .unwrap();

        assert_eq!(
            item,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "I need to think"
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_tool_call_as_assistant_message() {
        let item = ClaudeCodec.encode_tool_call(
            "toolu_123",
            "get_weather",
            r#"{"location":"San Francisco"}"#,
            &ProviderMeta::default(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_123",
                        "name": "get_weather",
                        "input": {
                            "location": "San Francisco"
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_tool_result_as_user_message() {
        let item = ClaudeCodec.encode_tool_call_result("toolu_123", "get_weather", "Sunny");

        assert_eq!(
            item,
            serde_json::json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": [
                            {
                                "type": "text",
                                "text": "Sunny"
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_hosted_tool_as_assistant_message() {
        let content = Some(r#"{"query":"weather"}"#.to_string());
        let item = ClaudeCodec
            .encode_hosted_tool(
                "web_search",
                &content,
                &ProviderMeta::default(),
                EncodeMode::SameProvider,
            )
            .unwrap();

        assert_eq!(
            item,
            serde_json::json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "server_tool_use",
                        "name": "web_search",
                        "input": {
                            "query": "weather"
                        }
                    }
                ]
            })
        );
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;

    #[test]
    fn decodes_output_text_delta() {
        let delta = ClaudeCodec
            .decode_output_text_delta(serde_json::json!({
                "type": "text_delta",
                "text": "Okay"
            }))
            .unwrap();

        assert_eq!(delta, "Okay");
    }

    #[test]
    fn decodes_reasoning_delta() {
        let delta = ClaudeCodec
            .decode_reasoning_delta(serde_json::json!({
                "type": "thinking_delta",
                "thinking": "I need to check the weather"
            }))
            .unwrap();

        assert_eq!(delta, "I need to check the weather");
    }

    #[test]
    fn decodes_text_output_item() {
        let item = ClaudeCodec
            .decode_output_item(serde_json::json!({
                "type": "text",
                "text": "Okay, let's check the weather.",
                "citations": []
            }))
            .unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::Message(ConversationMessage {
                    message: vec![MessageContentItem {
                        content: "Okay, let's check the weather.".to_string(),
                        provider_meta: HashMap::default(),
                    }],
                    provider_meta: [("citations".to_string(), serde_json::json!([]).to_string())]
                        .into(),
                })),
            }
        );
    }

    #[test]
    fn decodes_thinking_output_item() {
        let item = ClaudeCodec
            .decode_output_item(serde_json::json!({
                "type": "thinking",
                "thinking": "I need to check the weather",
                "signature": "example_signature"
            }))
            .unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::Reasoning(Reasoning {
                    reasoning: vec![SummaryItem {
                        content: "I need to check the weather".to_string(),
                        provider_meta: HashMap::default(),
                    }],
                    provider_meta: [(
                        "signature".to_string(),
                        serde_json::json!("example_signature").to_string()
                    )]
                    .into(),
                })),
            }
        );
    }

    #[test]
    fn decodes_tool_use_output_item() {
        let item = ClaudeCodec
            .decode_output_item(serde_json::json!({
                "type": "tool_use",
                "id": "toolu_01T1x1fJ34qAmk2tNTrN7Up6",
                "name": "get_weather",
                "input": r#"{"location": "San Francisco, CA"}"#,
                "extra": "preserve me"
            }))
            .unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::ToolCall(ToolCall {
                    call_id: "toolu_01T1x1fJ34qAmk2tNTrN7Up6".to_string(),
                    name: "get_weather".to_string(),
                    arguments: r#"{"location": "San Francisco, CA"}"#.to_string(),
                    provider_meta: [(
                        "extra".to_string(),
                        serde_json::json!("preserve me").to_string()
                    )]
                    .into(),
                })),
            }
        );
    }

    /// A zero-argument tool call never receives an input_json_delta, so the
    /// block still holds the `{}` object from content_block_start.
    #[test]
    fn decodes_zero_argument_tool_use_output_item() {
        let item = ClaudeCodec
            .decode_output_item(serde_json::json!({
                "type": "tool_use",
                "id": "toolu_01T1x1fJ34qAmk2tNTrN7Up6",
                "name": "no_args",
                "input": {}
            }))
            .unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::ToolCall(ToolCall {
                    call_id: "toolu_01T1x1fJ34qAmk2tNTrN7Up6".to_string(),
                    name: "no_args".to_string(),
                    arguments: "{}".to_string(),
                    provider_meta: HashMap::default(),
                })),
            }
        );
    }

    #[test]
    fn decodes_empty_string_input_identically_to_object_input() {
        let tool_use = |input: Value| {
            ClaudeCodec
                .decode_output_item(serde_json::json!({
                    "type": "tool_use",
                    "id": "toolu_01T1x1fJ34qAmk2tNTrN7Up6",
                    "name": "no_args",
                    "input": input
                }))
                .unwrap()
        };

        assert_eq!(
            tool_use(serde_json::json!({})),
            tool_use(serde_json::json!("{}"))
        );
    }

    #[test]
    fn decodes_server_tool_use_output_item() {
        let item = ClaudeCodec
            .decode_output_item(serde_json::json!({
                "type": "server_tool_use",
                "id": "srvtoolu_014hJH82Qum7Td6UV8gDXThB",
                "name": "web_search",
                "input": r#"{"query": "weather NYC today"}"#
            }))
            .unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::HostedTool(HostedTool {
                    function_type: "web_search".to_string(),
                    content: Some(r#"{"query": "weather NYC today"}"#.to_string()),
                    provider_meta: [(
                        "id".to_string(),
                        serde_json::json!("srvtoolu_014hJH82Qum7Td6UV8gDXThB").to_string()
                    )]
                    .into(),
                })),
            }
        );
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;

    #[test]
    fn redacted_thinking_round_trips() {
        let raw = serde_json::json!({
            "type": "redacted_thinking",
            "data": "EqgfCioIARgBIiQ3YTAwMjY1Mi1mZjM5"
        });
        let item = ClaudeCodec.decode_output_item(raw.clone()).unwrap();
        let encoded = ClaudeCodec
            .encode_conversation_item(&item, EncodeMode::SameProvider)
            .unwrap();
        assert_eq!(encoded["content"][0], raw);
    }

    #[test]
    fn web_search_tool_result_round_trips() {
        let raw = serde_json::json!({
            "type": "web_search_tool_result",
            "tool_use_id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
            "content": [
                {
                    "type": "web_search_result",
                    "url": "https://en.wikipedia.org/wiki/Claude_Shannon",
                    "title": "Claude Shannon - Wikipedia",
                    "encrypted_content": "EqgfCioIARgBIiQ3YTAwMjY1Mi1mZjM5",
                    "page_age": "April 30, 2025"
                }
            ]
        });
        let item = ClaudeCodec.decode_output_item(raw.clone()).unwrap();
        let encoded = ClaudeCodec
            .encode_conversation_item(&item, EncodeMode::SameProvider)
            .unwrap();
        assert_eq!(encoded["content"][0], raw);
    }

    #[test]
    fn server_tool_use_round_trips_input_as_object() {
        let raw = serde_json::json!({
            "type": "server_tool_use",
            "id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
            "name": "web_search",
            "input": r#"{"query": "weather NYC today"}"#
        });
        let item = ClaudeCodec.decode_output_item(raw).unwrap();
        let encoded = ClaudeCodec
            .encode_conversation_item(&item, EncodeMode::SameProvider)
            .unwrap();
        assert_eq!(
            encoded["content"][0],
            serde_json::json!({
                "type": "server_tool_use",
                "id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
                "name": "web_search",
                "input": { "query": "weather NYC today" }
            })
        );
    }
}
