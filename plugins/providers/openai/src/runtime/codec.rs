use std::collections::BTreeMap;

use log::warn;
use scry_provider_base::{
    ProviderDecoder, ProviderEncoder, ProviderError, ProviderMeta, Result, provider_meta,
    provider_meta_to_map,
};
use scry_provider_protocol::v1::{
    ConversationItem, ConversationMessage, EncodeMode, HostedTool, MessageContentItem, Reasoning,
    SummaryItem, ToolCall, conversation_item,
};
use scry_utils::Element;
use serde_json::Value;

pub struct CodexCodec;

impl ProviderEncoder for CodexCodec {
    fn encode_env_context(&self, envs: &BTreeMap<&'static str, String>) -> Value {
        let env_instruction = envs
            .iter()
            .fold(
                Element::new("environment_context"),
                |element, (key, value)| element.child(Element::new(*key).plain_text(value)),
            )
            .to_string();
        self.encode_user_prompt(&env_instruction)
    }

    fn encode_user_prompt(&self, prompt: &str) -> Value {
        serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": prompt }
            ]
        })
    }

    fn encode_message(
        &self,
        message: &[MessageContentItem],
        provider_meta: &ProviderMeta,
        encode_mode: EncodeMode,
    ) -> Value {
        let same_provider = matches!(encode_mode, EncodeMode::SameProvider);
        let mut item = provider_meta_to_map(provider_meta, same_provider);

        item.insert("type".to_string(), Value::String("message".to_string()));
        item.entry("role".to_string())
            .or_insert_with(|| Value::String("assistant".to_string()));
        item.insert(
            "content".to_string(),
            Value::Array(
                message
                    .iter()
                    .map(|message| {
                        let mut content =
                            provider_meta_to_map(&message.provider_meta, same_provider);

                        content
                            .entry("type".to_string())
                            .or_insert_with(|| Value::String("output_text".to_string()));

                        if content.get("type").and_then(Value::as_str) == Some("refusal") {
                            content.insert(
                                "refusal".to_string(),
                                Value::String(message.content.clone()),
                            );
                        } else {
                            content
                                .insert("text".to_string(), Value::String(message.content.clone()));
                        }

                        Value::Object(content)
                    })
                    .collect(),
            ),
        );

        Value::Object(item)
    }

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

        item.insert("type".to_string(), Value::String("reasoning".to_string()));
        item.insert(
            "summary".to_string(),
            Value::Array(
                content
                    .iter()
                    .map(|summary| {
                        let mut summary_item = provider_meta_to_map(&summary.provider_meta, true);

                        summary_item
                            .entry("type".to_string())
                            .or_insert_with(|| Value::String("summary_text".to_string()));
                        summary_item
                            .insert("text".to_string(), Value::String(summary.content.clone()));

                        Value::Object(summary_item)
                    })
                    .collect(),
            ),
        );

        Some(Value::Object(item))
    }

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

        item.insert(
            "type".to_string(),
            Value::String("function_call".to_string()),
        );
        item.insert("call_id".to_string(), Value::String(call_id.to_string()));
        item.insert("name".to_string(), Value::String(name.to_string()));
        item.insert(
            "arguments".to_string(),
            Value::String(arguments.to_string()),
        );

        Value::Object(item)
    }

    fn encode_tool_call_result(&self, call_id: &str, _name: &str, tool_output: &str) -> Value {
        serde_json::json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": tool_output,
        })
    }

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

        item.insert("type".to_string(), Value::String(function_type.to_string()));

        if let Some(content) = content {
            let field = match function_type {
                "web_search_call" => "action",
                _ => "content",
            };
            let content =
                serde_json::from_str(content).unwrap_or_else(|_| Value::String(content.clone()));
            item.insert(field.to_string(), content);
        }

        Some(Value::Object(item))
    }

    fn encode_unknown(
        &self,
        _provider_meta: &ProviderMeta,
        _encode_mode: EncodeMode,
    ) -> Option<Value> {
        None
    }
}

impl ProviderDecoder for CodexCodec {
    /// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_text.delta
    fn decode_output_text_delta(&self, data: Value) -> Result<String> {
        data.get("delta")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| ProviderError::Other("missing output text delta field".into()))
    }

    /// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.reasoning_summary_text.delta
    /// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.reasoning_summary_text.done
    fn decode_reasoning_delta(&self, data: Value) -> Result<String> {
        data.get("delta")
            .or_else(|| data.get("text"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| ProviderError::Other("missing reasoning delta or text field".into()))
    }

    /// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_item.done
    /// https://developers.openai.com/api/reference/resources/responses#(resource)%20responses%20%3E%20(model)%20response_output_item%20%3E%20(schema)
    fn decode_output_item(&self, data: Value) -> Result<ConversationItem> {
        let item = data
            .get("item")
            .ok_or_else(|| ProviderError::Other("missing output item".into()))?;
        let response_type = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Other("missing output item type".into()))?;

        match response_type {
            "message" => decode_message_item(item),
            "reasoning" => decode_reasoning_item(item),
            "function_call" => decode_function_call_item(item),
            "web_search_call" => decode_web_search_call_item(item),
            _ => {
                warn!("unknown response type {}", response_type);
                decode_hosted_tool_item(response_type, item)
            },
        }
    }
}

fn decode_message_item(item: &Value) -> Result<ConversationItem> {
    let message = item
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .map(|part| MessageContentItem {
                    content: part
                        .get("text")
                        .or_else(|| part.get("refusal"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    provider_meta: provider_meta(part, &["text", "refusal"]),
                })
                .collect()
        })
        .unwrap_or_default();
    let provider_meta = provider_meta(item, &["type", "content"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::Message(ConversationMessage {
            message,
            provider_meta,
        })),
    })
}

fn decode_reasoning_item(item: &Value) -> Result<ConversationItem> {
    let reasoning = item
        .get("summary")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .map(|part| SummaryItem {
                    content: part
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    provider_meta: provider_meta(part, &["text"]),
                })
                .collect()
        })
        .unwrap_or_default();
    let provider_meta = provider_meta(item, &["type", "summary"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::Reasoning(Reasoning {
            reasoning,
            provider_meta,
        })),
    })
}

fn decode_function_call_item(item: &Value) -> Result<ConversationItem> {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call id".into()))?
        .to_string();
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call name".into()))?
        .to_string();
    let arguments = item
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing function call arguments".into()))?
        .to_string();
    let provider_meta = provider_meta(item, &["type", "call_id", "name", "arguments"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::ToolCall(ToolCall {
            call_id,
            name,
            arguments,
            provider_meta,
        })),
    })
}

fn decode_web_search_call_item(item: &Value) -> Result<ConversationItem> {
    let content = item.get("action").map(Value::to_string);
    let provider_meta = provider_meta(item, &["type", "action"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::HostedTool(HostedTool {
            function_type: "web_search_call".to_string(),
            content,
            provider_meta,
        })),
    })
}

fn decode_hosted_tool_item(response_type: &str, item: &Value) -> Result<ConversationItem> {
    let provider_meta = provider_meta(item, &["type"]);

    Ok(ConversationItem {
        item: Some(conversation_item::Item::HostedTool(HostedTool {
            function_type: response_type.to_string(),
            content: None,
            provider_meta,
        })),
    })
}

#[cfg(test)]
mod encoder_tests {
    use scry_provider_protocol::v1::EncodeMode;

    use super::*;

    #[test]
    fn encodes_env_context() {
        let item = CodexCodec.encode_env_context(&BTreeMap::from([
            ("os", "linux".to_string()),
            ("os_family", "unix".to_string()),
            ("arch", "x86_64".to_string()),
            ("home", "/home/example".to_string()),
            ("shell", "/bin/bash".to_string()),
        ]));

        assert_eq!(
            item,
            serde_json::json!({
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "<environment_context>\n<arch>x86_64</arch>\n<home>/home/example</home>\n<os>linux</os>\n<os_family>unix</os_family>\n<shell>/bin/bash</shell>\n</environment_context>"
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_user_prompt() {
        let item = CodexCodec.encode_user_prompt("example prompt");

        assert_eq!(
            item,
            serde_json::json!({
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "example prompt",
                    }
                ],
            })
        );
    }

    #[test]
    fn encodes_same_provider_output_text_message() {
        let item = CodexCodec.encode_message(
            &[MessageContentItem {
                content: "example output".to_string(),
                provider_meta: [
                    ("annotations".to_string(), serde_json::json!([]).to_string()),
                    (
                        "type".to_string(),
                        Value::String("output_text".to_string()).to_string(),
                    ),
                ]
                .into(),
            }],
            &[
                (
                    "id".to_string(),
                    Value::String("msg_123".to_string()).to_string(),
                ),
                (
                    "role".to_string(),
                    Value::String("assistant".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::SameProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "id": "msg_123",
                "status": "completed",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "example output",
                        "annotations": []
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_same_provider_refusal_message() {
        let item = CodexCodec.encode_message(
            &[MessageContentItem {
                content: "example refusal".to_string(),
                provider_meta: [(
                    "type".to_string(),
                    Value::String("refusal".to_string()).to_string(),
                )]
                .into(),
            }],
            &[
                (
                    "id".to_string(),
                    Value::String("msg_123".to_string()).to_string(),
                ),
                (
                    "role".to_string(),
                    Value::String("assistant".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::SameProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "id": "msg_123",
                "status": "completed",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "refusal",
                        "refusal": "example refusal"
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_cross_provider_message() {
        let item = CodexCodec.encode_message(
            &[MessageContentItem {
                content: "example output".to_string(),
                provider_meta: [
                    ("annotations".to_string(), serde_json::json!([]).to_string()),
                    (
                        "type".to_string(),
                        Value::String("output_text".to_string()).to_string(),
                    ),
                ]
                .into(),
            }],
            &[
                (
                    "id".to_string(),
                    Value::String("msg_123".to_string()).to_string(),
                ),
                (
                    "role".to_string(),
                    Value::String("assistant".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
                (
                    "random_other_provider".to_string(),
                    Value::String("random_other_provider".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "example output"
                    }
                ]
            })
        );
    }

    #[test]
    fn encodes_same_provider_reasoning() {
        let item = CodexCodec.encode_reasoning(
            &[SummaryItem {
                content: "example summary".to_string(),
                provider_meta: [(
                    "type".to_string(),
                    Value::String("summary_text".to_string()).to_string(),
                )]
                .into(),
            }],
            &[
                ("content".to_string(), serde_json::json!([]).to_string()),
                (
                    "encrypted_content".to_string(),
                    Value::String("encrypted_reasoning".to_string()).to_string(),
                ),
                (
                    "id".to_string(),
                    Value::String("rs_123".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::SameProvider,
        );

        assert_eq!(
            item,
            Some(serde_json::json!({
                "content": [],
                "encrypted_content": "encrypted_reasoning",
                "id": "rs_123",
                "summary": [
                    {
                        "type": "summary_text",
                        "text": "example summary"
                    }
                ],
                "type": "reasoning"
            }))
        );
    }

    #[test]
    fn encodes_cross_provider_reasoning() {
        let item = CodexCodec.encode_reasoning(
            &[SummaryItem {
                content: "example summary".into(),
                provider_meta: [(
                    "type".to_string(),
                    Value::String("summary_text".to_string()).to_string(),
                )]
                .into(),
            }],
            &[
                ("content".to_string(), serde_json::json!([]).to_string()),
                (
                    "id".to_string(),
                    Value::String("rs_123".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(item, None);
    }

    #[test]
    fn encodes_same_provider_tool_call() {
        let item = CodexCodec.encode_tool_call(
            "call_123",
            "example_tool",
            "{\"query\":\"example search\"}",
            &[
                (
                    "id".to_string(),
                    Value::String("fc_123".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::SameProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "arguments": "{\"query\":\"example search\"}",
                "call_id": "call_123",
                "id": "fc_123",
                "name": "example_tool",
                "status": "completed",
                "type": "function_call"
            })
        );
    }

    #[test]
    fn encodes_cross_provider_tool_call() {
        let item = CodexCodec.encode_tool_call(
            "call_123",
            "example_tool",
            "{\"query\":\"example search\"}",
            &[
                (
                    "id".to_string(),
                    Value::String("fc_123".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(
            item,
            serde_json::json!({
                "arguments": "{\"query\":\"example search\"}",
                "call_id": "call_123",
                "name": "example_tool",
                "type": "function_call"
            })
        );
    }

    #[test]
    fn encodes_tool_call_result() {
        let item = CodexCodec.encode_tool_call_result("call_123", "example_tool", "example output");

        assert_eq!(
            item,
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_123",
                "output": "example output",
            })
        );
    }

    #[test]
    fn encodes_same_provider_hosted_tool() {
        let item = CodexCodec.encode_hosted_tool(
            "web_search_call",
            &Some(r#"{"queries":["example query"],"type":"search"}"#.to_string()),
            &[
                (
                    "id".to_string(),
                    Value::String("ws_123".to_string()).to_string(),
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string(),
                ),
            ]
            .into(),
            EncodeMode::SameProvider,
        );

        assert_eq!(
            item,
            Some(serde_json::json!({
                "action": {
                    "queries": ["example query"],
                    "type": "search"
                },
                "id": "ws_123",
                "status": "completed",
                "type": "web_search_call"
            }))
        );
    }

    #[test]
    fn encodes_cross_provider_hosted_tool() {
        let item = CodexCodec.encode_hosted_tool(
            "web_search_call",
            &Some(r#"{"queries":["example query"],"type":"search"}"#.to_string()),
            &[
                ("id".to_string(), "ws_123".to_string()),
                ("status".to_string(), "completed".to_string()),
            ]
            .into(),
            EncodeMode::CrossProvider,
        );

        assert_eq!(item, None);
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;

    fn json(data: &str) -> Value {
        serde_json::from_str(data).unwrap()
    }

    #[test]
    fn decodes_output_text_delta() {
        let payload = r#"{
          "type": "response.output_text.delta",
          "item_id": "msg_123",
          "output_index": 0,
          "content_index": 0,
          "delta": "example output",
          "sequence_number": 1
        }"#;

        let delta = CodexCodec.decode_output_text_delta(json(payload)).unwrap();

        assert_eq!(delta, "example output");
    }

    #[test]
    fn decodes_reasoning_delta() {
        let expected = "example reasoning summary";
        let payload = r#"{
          "type": "response.reasoning_summary_text.delta",
          "item_id": "rs_123",
          "output_index": 0,
          "summary_index": 0,
          "delta": "example reasoning summary",
          "sequence_number": 1
        }"#;

        let delta = CodexCodec.decode_reasoning_delta(json(payload)).unwrap();

        assert_eq!(delta, expected);
    }

    #[test]
    fn decodes_reasoning_delta_from_done_text() {
        let expected = "example final reasoning summary";
        let payload = r#"{
          "type": "response.reasoning_summary_text.done",
          "item_id": "rs_123",
          "output_index": 0,
          "summary_index": 0,
          "text": "example final reasoning summary",
          "sequence_number": 1
        }"#;

        let delta = CodexCodec.decode_reasoning_delta(json(payload)).unwrap();

        assert_eq!(delta, expected);
    }

    #[test]
    fn decodes_message_item() {
        let item = serde_json::json!({
          "id": "msg_123",
          "status": "completed",
          "type": "message",
          "role": "assistant",
          "content": [
            {
              "type": "output_text",
              "text": "example output",
              "annotations": []
            },
            {
              "type": "refusal",
              "refusal": "example refusal"
            }
          ]
        });

        let item = decode_message_item(&item).unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::Message(ConversationMessage {
                    message: vec![
                        MessageContentItem {
                            content: "example output".to_string(),
                            provider_meta: [
                                ("annotations".to_string(), serde_json::json!([]).to_string()),
                                (
                                    "type".to_string(),
                                    Value::String("output_text".to_string()).to_string()
                                ),
                            ]
                            .into()
                        },
                        MessageContentItem {
                            content: "example refusal".to_string(),
                            provider_meta: [(
                                "type".to_string(),
                                Value::String("refusal".to_string()).to_string()
                            )]
                            .into()
                        },
                    ],
                    provider_meta: [
                        (
                            "id".to_string(),
                            Value::String("msg_123".to_string()).to_string()
                        ),
                        (
                            "role".to_string(),
                            Value::String("assistant".to_string()).to_string()
                        ),
                        (
                            "status".to_string(),
                            Value::String("completed".to_string()).to_string()
                        ),
                    ]
                    .into()
                })),
            }
        );
    }

    #[test]
    fn decodes_reasoning_item() {
        let item = serde_json::json!({
          "content": [],
          "encrypted_content": "encrypted_reasoning",
          "id": "rs_123",
          "summary": [
            {
              "type": "summary_text",
              "text": "first summary"
            },
            {
              "type": "summary_text",
              "text": "second summary"
            }
          ],
          "type": "reasoning"
        });

        let item = decode_reasoning_item(&item).unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::Reasoning(Reasoning {
                    reasoning: vec![
                        SummaryItem {
                            content: "first summary".to_string(),
                            provider_meta: [(
                                "type".to_string(),
                                Value::String("summary_text".to_string()).to_string()
                            )]
                            .into()
                        },
                        SummaryItem {
                            content: "second summary".to_string(),
                            provider_meta: [(
                                "type".to_string(),
                                Value::String("summary_text".to_string()).to_string()
                            )]
                            .into()
                        },
                    ],
                    provider_meta: [
                        ("content".to_string(), serde_json::json!([]).to_string()),
                        (
                            "encrypted_content".to_string(),
                            Value::String("encrypted_reasoning".to_string()).to_string()
                        ),
                        (
                            "id".to_string(),
                            Value::String("rs_123".to_string()).to_string()
                        ),
                    ]
                    .into()
                })),
            }
        );
    }

    #[test]
    fn decodes_function_call_output_item() {
        let payload = r#"{
          "type": "response.output_item.done",
          "output_index": 0,
          "item": {
            "arguments": "{\"query\":\"example search\"}",
            "call_id": "call_123",
            "id": "fc_123",
            "name": "example_tool",
            "status": "completed",
            "type": "function_call"
          },
          "sequence_number": 1
        }"#;

        let item = CodexCodec.decode_output_item(json(payload)).unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::ToolCall(ToolCall {
                    call_id: "call_123".to_string(),
                    name: "example_tool".to_string(),
                    arguments: "{\"query\":\"example search\"}".to_string(),
                    provider_meta: [
                        (
                            "id".to_string(),
                            Value::String("fc_123".to_string()).to_string()
                        ),
                        (
                            "status".to_string(),
                            Value::String("completed".to_string()).to_string()
                        ),
                    ]
                    .into()
                })),
            }
        );
    }

    #[test]
    fn decodes_web_search_call_output_item() {
        let payload = r#"{
          "type": "response.output_item.done",
          "output_index": 0,
          "item": {
            "action": {
              "queries": ["example query"],
              "type": "search"
            },
            "id": "ws_123",
            "status": "completed",
            "type": "web_search_call"
          },
          "sequence_number": 1
        }"#;

        let item = CodexCodec.decode_output_item(json(payload)).unwrap();

        let Some(conversation_item::Item::HostedTool(HostedTool {
            function_type,
            content,
            provider_meta,
        })) = item.item
        else {
            panic!("expected hosted tool item");
        };

        assert_eq!(function_type, "web_search_call");
        assert_eq!(
            serde_json::from_str::<Value>(&content.unwrap()).unwrap(),
            serde_json::json!({
                "queries": ["example query"],
                "type": "search",
            })
        );
        assert_eq!(
            provider_meta,
            [
                (
                    "id".to_string(),
                    Value::String("ws_123".to_string()).to_string()
                ),
                (
                    "status".to_string(),
                    Value::String("completed".to_string()).to_string()
                ),
            ]
            .into()
        );
    }

    #[test]
    fn decodes_unknown_output_item_as_hosted_tool() {
        let payload = r#"{
          "type": "response.output_item.done",
          "output_index": 0,
          "item": {
            "call_id": "call_123",
            "id": "ctc_123",
            "input": "example custom tool input",
            "name": "example_custom_tool",
            "namespace": "example_namespace",
            "type": "custom_tool_call"
          },
          "sequence_number": 1
        }"#;

        let item = CodexCodec.decode_output_item(json(payload)).unwrap();

        assert_eq!(
            item,
            ConversationItem {
                item: Some(conversation_item::Item::HostedTool(HostedTool {
                    function_type: "custom_tool_call".to_string(),
                    content: None,
                    provider_meta: [
                        (
                            "call_id".to_string(),
                            Value::String("call_123".to_string()).to_string()
                        ),
                        (
                            "id".to_string(),
                            Value::String("ctc_123".to_string()).to_string()
                        ),
                        (
                            "input".to_string(),
                            Value::String("example custom tool input".to_string()).to_string()
                        ),
                        (
                            "name".to_string(),
                            Value::String("example_custom_tool".to_string()).to_string()
                        ),
                        (
                            "namespace".to_string(),
                            Value::String("example_namespace".to_string()).to_string()
                        ),
                    ]
                    .into(),
                })),
            }
        );
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;

    fn assert_roundtrip(item: Value) {
        let decoded = CodexCodec
            .decode_output_item(serde_json::json!({ "item": item }))
            .unwrap();
        let encoded = CodexCodec
            .encode_conversation_item(&decoded, EncodeMode::SameProvider)
            .expect("same-provider encode must yield an item");
        assert_eq!(encoded, item);
    }

    #[test]
    fn message_item_roundtrip() {
        assert_roundtrip(serde_json::json!({
            "id": "msg_123",
            "status": "completed",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "output_text",
                    "text": "example output",
                    "annotations": []
                },
                {
                    "type": "refusal",
                    "refusal": "example refusal"
                }
            ]
        }));
    }

    #[test]
    fn reasoning_item_roundtrip() {
        assert_roundtrip(serde_json::json!({
            "content": [],
            "encrypted_content": "encrypted_reasoning",
            "id": "rs_123",
            "summary": [
                {
                    "type": "summary_text",
                    "text": "first summary"
                },
                {
                    "type": "summary_text",
                    "text": "second summary"
                }
            ],
            "type": "reasoning"
        }));
    }

    #[test]
    fn function_call_item_roundtrip() {
        assert_roundtrip(serde_json::json!({
            "arguments": "{\"query\":\"example search\"}",
            "call_id": "call_123",
            "id": "fc_123",
            "name": "example_tool",
            "status": "completed",
            "type": "function_call"
        }));
    }

    #[test]
    fn web_search_call_item_roundtrip() {
        assert_roundtrip(serde_json::json!({
            "action": {
                "queries": ["example query"],
                "type": "search"
            },
            "id": "ws_123",
            "status": "completed",
            "type": "web_search_call"
        }));
    }

    #[test]
    fn unknown_item_roundtrip_as_hosted_tool() {
        assert_roundtrip(serde_json::json!({
            "call_id": "call_123",
            "id": "ctc_123",
            "input": "example custom tool input",
            "name": "example_custom_tool",
            "namespace": "example_namespace",
            "type": "custom_tool_call"
        }));
    }
}
