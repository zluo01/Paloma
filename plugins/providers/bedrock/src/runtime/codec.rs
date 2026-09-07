use std::collections::HashMap;

use aws_sdk_bedrockruntime::types::{
    ContentBlock, ConversationRole, Message, ReasoningContentBlock, ReasoningTextBlock,
    SystemContentBlock, Tool, ToolConfiguration, ToolInputSchema, ToolResultBlock,
    ToolResultContentBlock, ToolSpecification, ToolUseBlock,
};
use aws_smithy_types::{Blob, Document, Number, base64};
use log::warn;
use paloma_provider_base::{ENVIRONMENT_CONTEXT, ProviderError, Result, provider_meta_to_map};
use paloma_provider_protocol::v1::{ChatRequest, conversation_item::Item};
use paloma_utils::Element;
use serde_json::Value;

use crate::{
    constant::PROVIDER_ID,
    runtime::models::{DEFAULT_EFFORT, supports_thinking},
};

pub(super) const META_SIGNATURE: &str = "signature";
pub(super) const META_REDACTED_CONTENT: &str = "redactedContent";

const THINKING_BUDGET_TOKENS: u64 = 4096;

pub(super) fn construct_system_prompt(request: &ChatRequest) -> Vec<SystemContentBlock> {
    let env_context = ENVIRONMENT_CONTEXT
        .iter()
        .fold(
            Element::new("environment_context"),
            |element, (key, value)| element.child(Element::new(*key).plain_text(value)),
        )
        .to_string();
    vec![
        SystemContentBlock::Text(request.instruction.clone()),
        SystemContentBlock::Text(env_context),
    ]
}

pub(super) fn construct_reasoning_config(request: &ChatRequest) -> Option<Document> {
    if request.effort.is_empty() {
        return None;
    }
    if DEFAULT_EFFORT == request.effort {
        return supports_thinking(&request.model).then(|| {
            Document::Object(HashMap::from([(
                "thinking".to_string(),
                Document::Object(HashMap::from([
                    ("type".to_string(), Document::String("enabled".to_string())),
                    (
                        "budget_tokens".to_string(),
                        Document::Number(Number::PosInt(THINKING_BUDGET_TOKENS)),
                    ),
                ])),
            )]))
        });
    }

    Some(Document::Object(HashMap::from([
        (
            "thinking".to_string(),
            Document::Object(HashMap::from([(
                "type".to_string(),
                Document::String("adaptive".to_string()),
            )])),
        ),
        (
            "output_config".to_string(),
            Document::Object(HashMap::from([(
                "effort".to_string(),
                Document::String(request.effort.clone()),
            )])),
        ),
    ])))
}

pub(super) fn construct_messages(request: &ChatRequest) -> Result<Vec<Message>> {
    let mut folded: Vec<(ConversationRole, Vec<ContentBlock>)> = Vec::new();

    for message in &request.messages {
        let same_provider = message.provider_id == PROVIDER_ID;
        let Some(item) = message.item.as_ref().and_then(|i| i.item.as_ref()) else {
            continue;
        };
        let Some((role, block)) = construct_content_block(item, same_provider)? else {
            continue;
        };

        match folded.last_mut() {
            Some((last_role, blocks)) if *last_role == role => blocks.push(block),
            _ => folded.push((role, vec![block])),
        }
    }

    folded
        .into_iter()
        .map(|(role, blocks)| {
            Message::builder()
                .role(role)
                .set_content(Some(blocks))
                .build()
                .map_err(|e| ProviderError::Other(format!("fail to build converse message: {e}")))
        })
        .collect()
}

fn construct_content_block(
    item: &Item,
    same_provider: bool,
) -> Result<Option<(ConversationRole, ContentBlock)>> {
    let block = match item {
        Item::UserPrompt(prompt) => Some((
            ConversationRole::User,
            ContentBlock::Text(prompt.prompt.clone()),
        )),
        Item::Message(message) => Some((
            ConversationRole::Assistant,
            ContentBlock::Text(message.message.iter().map(|m| m.content.as_str()).collect()),
        )),
        Item::Reasoning(reasoning) if same_provider => {
            let meta = provider_meta_to_map(&reasoning.provider_meta, true);
            let mut text = ReasoningTextBlock::builder().text(
                reasoning
                    .reasoning
                    .iter()
                    .map(|r| r.content.as_str())
                    .collect::<String>(),
            );
            if let Some(signature) = meta.get(META_SIGNATURE).and_then(Value::as_str) {
                text = text.signature(signature);
            }
            let block = ReasoningContentBlock::ReasoningText(text.build().map_err(|e| {
                ProviderError::Other(format!("fail to build reasoning block: {e}"))
            })?);
            Some((
                ConversationRole::Assistant,
                ContentBlock::ReasoningContent(block),
            ))
        },
        Item::Unknown(unknown) if same_provider => {
            let meta = provider_meta_to_map(&unknown.provider_meta, true);
            let Some(redacted) = meta.get(META_REDACTED_CONTENT).and_then(Value::as_str) else {
                return Ok(None);
            };
            let bytes = base64::decode(redacted).map_err(|e| {
                ProviderError::Other(format!("malformed redacted reasoning content: {e}"))
            })?;
            Some((
                ConversationRole::Assistant,
                ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(Blob::new(
                    bytes,
                ))),
            ))
        },
        Item::ToolCall(call) => {
            let input = serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| {
                warn!(
                    "fail to parse tool call arguments as JSON: {}",
                    call.arguments
                );
                Value::Object(Default::default())
            });
            let block = ToolUseBlock::builder()
                .tool_use_id(&call.call_id)
                .name(&call.name)
                .input(json_to_document(&input))
                .build()
                .map_err(|e| ProviderError::Other(format!("fail to build tool use block: {e}")))?;
            Some((ConversationRole::Assistant, ContentBlock::ToolUse(block)))
        },
        Item::ToolResult(result) => {
            let block = ToolResultBlock::builder()
                .tool_use_id(&result.call_id)
                .content(ToolResultContentBlock::Text(result.output.clone()))
                .build()
                .map_err(|e| {
                    ProviderError::Other(format!("fail to build tool result block: {e}"))
                })?;
            Some((ConversationRole::User, ContentBlock::ToolResult(block)))
        },
        Item::Reasoning(_) | Item::Unknown(_) | Item::HostedTool(_) => None,
    };
    Ok(block)
}

pub(super) fn construct_tool_config(request: &ChatRequest) -> Result<Option<ToolConfiguration>> {
    let tools: Vec<Tool> = request
        .tools
        .iter()
        .filter_map(|t| {
            let parameters = match serde_json::from_str::<Value>(&t.parameters) {
                Ok(p) => p,
                Err(e) => {
                    warn!(
                        "tool {:?} has malformed parameters JSON ({e}); skipping tool.",
                        t.name
                    );
                    return None;
                },
            };
            let spec = ToolSpecification::builder()
                .name(&t.name)
                .description(&t.description)
                .input_schema(ToolInputSchema::Json(json_to_document(&parameters)))
                .build();
            match spec {
                Ok(spec) => Some(Tool::ToolSpec(spec)),
                Err(e) => {
                    warn!(
                        "fail to build tool spec for {:?}: {e}; skipping tool.",
                        t.name
                    );
                    None
                },
            }
        })
        .collect();

    // Converse rejects an empty tool list
    if tools.is_empty() {
        return Ok(None);
    }

    ToolConfiguration::builder()
        .set_tools(Some(tools))
        .build()
        .map(Some)
        .map_err(|e| ProviderError::Other(format!("fail to build tool configuration: {e}")))
}

fn json_to_document(value: &Value) -> Document {
    match value {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(*b),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Document::Number(Number::PosInt(u))
            } else if let Some(i) = n.as_i64() {
                Document::Number(Number::NegInt(i))
            } else if let Some(f) = n.as_f64() {
                Document::Number(Number::Float(f))
            } else {
                Document::Null
            }
        },
        Value::String(s) => Document::String(s.clone()),
        Value::Array(items) => Document::Array(items.iter().map(json_to_document).collect()),
        Value::Object(map) => Document::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_document(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use paloma_provider_protocol::v1::{
        ChatRequestMessage, ConversationItem, ConversationMessage, MessageContentItem, Reasoning,
        SummaryItem, ToolCall, ToolDefinition, ToolResult, Unknown, UserPrompt,
    };

    use super::*;

    const ADAPTIVE_THINKING_MODEL: &str = "us.anthropic.claude-opus-5";
    const THINKING_MODEL: &str = "us.anthropic.claude-opus-4-5-20251101-v1:0";
    const DEFAULT_MODEL: &str = "deepseek.v3.2";

    fn request(model: &str, effort: &str) -> ChatRequest {
        ChatRequest {
            session_id: "session".into(),
            instruction: "be brief".into(),
            model: model.into(),
            effort: effort.into(),
            messages: vec![],
            tools: vec![],
        }
    }

    fn message(provider_id: &str, item: Item) -> ChatRequestMessage {
        ChatRequestMessage {
            provider_id: provider_id.into(),
            backend_id: String::new(),
            item: Some(ConversationItem { item: Some(item) }),
        }
    }

    fn user_prompt(prompt: &str) -> Item {
        Item::UserPrompt(UserPrompt {
            prompt: prompt.into(),
        })
    }

    fn assistant_text(text: &str) -> Item {
        Item::Message(ConversationMessage {
            message: vec![MessageContentItem {
                content: text.into(),
                provider_meta: Default::default(),
            }],
            provider_meta: Default::default(),
        })
    }

    fn reasoning(text: &str, signature: &str) -> Item {
        Item::Reasoning(Reasoning {
            reasoning: vec![SummaryItem {
                content: text.into(),
                provider_meta: Default::default(),
            }],
            // provider_meta values are JSON text, as stream.rs writes them
            provider_meta: [(META_SIGNATURE.to_string(), format!("{signature:?}"))].into(),
        })
    }

    fn redacted(bytes: &[u8]) -> Item {
        Item::Unknown(Unknown {
            provider_meta: [(
                META_REDACTED_CONTENT.to_string(),
                format!("{:?}", base64::encode(bytes)),
            )]
            .into(),
        })
    }

    fn tool_call(call_id: &str, arguments: &str) -> Item {
        Item::ToolCall(ToolCall {
            call_id: call_id.into(),
            name: "get_weather".into(),
            arguments: arguments.into(),
            provider_meta: Default::default(),
        })
    }

    fn tool_result(call_id: &str, output: &str) -> Item {
        Item::ToolResult(ToolResult {
            call_id: call_id.into(),
            name: "get_weather".into(),
            output: output.into(),
        })
    }

    fn tool(name: &str, parameters: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: format!("{name} description"),
            parameters: parameters.into(),
        }
    }

    fn field<'a>(document: &'a Document, key: &str) -> &'a Document {
        document.as_object().unwrap().get(key).unwrap()
    }

    mod reasoning_config {
        use super::*;

        #[test]
        fn empty_effort_produce_no_config() {
            for model in [ADAPTIVE_THINKING_MODEL, THINKING_MODEL, DEFAULT_MODEL] {
                assert!(construct_reasoning_config(&request(model, "")).is_none());
            }
        }

        #[test]
        fn default_effort_gives_thinking_only_to_thinking_models() {
            let config =
                construct_reasoning_config(&request(THINKING_MODEL, DEFAULT_EFFORT)).unwrap();
            let thinking = field(&config, "thinking");
            assert_eq!(field(thinking, "type").as_string(), Some("enabled"));
            assert_eq!(
                field(thinking, "budget_tokens").as_number(),
                Some(&Number::PosInt(THINKING_BUDGET_TOKENS))
            );
            assert!(config.as_object().unwrap().get("output_config").is_none());

            for model in [ADAPTIVE_THINKING_MODEL, DEFAULT_MODEL] {
                assert!(construct_reasoning_config(&request(model, DEFAULT_EFFORT)).is_none());
            }
        }

        #[test]
        fn real_effort_gives_adaptive_thinking() {
            let config =
                construct_reasoning_config(&request(ADAPTIVE_THINKING_MODEL, "xhigh")).unwrap();
            assert_eq!(
                field(field(&config, "thinking"), "type").as_string(),
                Some("adaptive")
            );
            assert_eq!(
                field(field(&config, "output_config"), "effort").as_string(),
                Some("xhigh")
            );
        }
    }

    mod messages {
        use super::*;

        #[test]
        fn consecutive_assistant_items_become_one_message_and_tool_result_becomes_user_message() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.messages = vec![
                message(PROVIDER_ID, user_prompt("weather in Paris?")),
                message(PROVIDER_ID, reasoning("need the tool", "sig")),
                message(PROVIDER_ID, assistant_text("Let me check.")),
                message(PROVIDER_ID, tool_call("call-1", r#"{"location":"Paris"}"#)),
                message(PROVIDER_ID, tool_result("call-1", "88F")),
            ];

            let messages = construct_messages(&request).unwrap();

            assert_eq!(messages.len(), 3);
            assert_eq!(*messages[0].role(), ConversationRole::User);
            assert_eq!(
                messages[0].content()[0].as_text().unwrap(),
                "weather in Paris?"
            );

            let assistant = &messages[1];
            assert_eq!(*assistant.role(), ConversationRole::Assistant);
            assert_eq!(assistant.content().len(), 3);
            let thinking = assistant.content()[0]
                .as_reasoning_content()
                .unwrap()
                .as_reasoning_text()
                .unwrap();
            assert_eq!(thinking.text(), "need the tool");
            assert_eq!(thinking.signature(), Some("sig"));
            assert_eq!(assistant.content()[1].as_text().unwrap(), "Let me check.");
            let tool_use = assistant.content()[2].as_tool_use().unwrap();
            assert_eq!(tool_use.tool_use_id(), "call-1");
            assert_eq!(tool_use.name(), "get_weather");
            assert_eq!(
                field(tool_use.input(), "location").as_string(),
                Some("Paris")
            );

            let user = &messages[2];
            assert_eq!(*user.role(), ConversationRole::User);
            let result = user.content()[0].as_tool_result().unwrap();
            assert_eq!(result.tool_use_id(), "call-1");
            assert_eq!(result.content()[0].as_text().unwrap(), "88F");
        }

        #[test]
        fn items_from_another_provider_keep_text_and_lose_reasoning() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.messages = vec![
                message(PROVIDER_ID, user_prompt("question")),
                message("OpenAI", reasoning("foreign thinking", "sig")),
                message("OpenAI", redacted(b"foreign")),
                message("OpenAI", assistant_text("foreign answer")),
            ];

            let messages = construct_messages(&request).unwrap();

            assert_eq!(messages.len(), 2);
            assert_eq!(messages[1].content().len(), 1);
            assert_eq!(
                messages[1].content()[0].as_text().unwrap(),
                "foreign answer"
            );
        }

        #[test]
        fn own_redacted_reasoning_is_sent_back_as_the_same_bytes() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.messages = vec![
                message(PROVIDER_ID, user_prompt("question")),
                message(PROVIDER_ID, redacted(b"opaque bytes")),
                message(PROVIDER_ID, assistant_text("answer")),
            ];

            let messages = construct_messages(&request).unwrap();

            let blob = messages[1].content()[0]
                .as_reasoning_content()
                .unwrap()
                .as_redacted_content()
                .unwrap();
            assert_eq!(blob.as_ref(), b"opaque bytes");
            assert_eq!(messages[1].content()[1].as_text().unwrap(), "answer");
        }

        #[test]
        fn own_unknown_item_without_redacted_content_is_skipped() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.messages = vec![
                message(PROVIDER_ID, user_prompt("question")),
                message(
                    PROVIDER_ID,
                    Item::Unknown(Unknown {
                        provider_meta: Default::default(),
                    }),
                ),
            ];

            let messages = construct_messages(&request).unwrap();

            assert_eq!(messages.len(), 1);
        }

        #[test]
        fn tool_call_with_invalid_json_arguments_is_sent_with_empty_input() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.messages = vec![
                message(PROVIDER_ID, user_prompt("question")),
                message(PROVIDER_ID, tool_call("call-1", "not json")),
            ];

            let messages = construct_messages(&request).unwrap();

            let tool_use = messages[1].content()[0].as_tool_use().unwrap();
            assert!(tool_use.input().as_object().unwrap().is_empty());
        }
    }

    mod tool_config {
        use super::*;

        #[test]
        fn request_without_tools_gets_no_tool_config() {
            assert!(
                construct_tool_config(&request(ADAPTIVE_THINKING_MODEL, "high"))
                    .unwrap()
                    .is_none()
            );
        }

        #[test]
        fn tool_definition_maps_to_tool_spec_with_name_description_and_json_schema() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.tools = vec![tool(
                "get_weather",
                r#"{"type":"object","properties":{"location":{"type":"string"}},"required":["location"]}"#,
            )];

            let config = construct_tool_config(&request).unwrap().unwrap();

            assert_eq!(config.tools().len(), 1);
            let spec = config.tools()[0].as_tool_spec().unwrap();
            assert_eq!(spec.name(), "get_weather");
            assert_eq!(spec.description(), Some("get_weather description"));
            let schema = spec.input_schema().unwrap().as_json().unwrap();
            assert_eq!(field(schema, "type").as_string(), Some("object"));
            assert_eq!(
                field(schema, "required").as_array().unwrap()[0].as_string(),
                Some("location")
            );
        }

        #[test]
        fn tool_with_invalid_json_schema_is_skipped_and_no_valid_tools_gives_no_config() {
            let mut request = request(ADAPTIVE_THINKING_MODEL, "high");
            request.tools = vec![
                tool("broken", "not json"),
                tool("get_weather", r#"{"type":"object"}"#),
            ];

            let config = construct_tool_config(&request).unwrap().unwrap();
            assert_eq!(config.tools().len(), 1);
            assert_eq!(
                config.tools()[0].as_tool_spec().unwrap().name(),
                "get_weather"
            );

            request.tools = vec![tool("broken", "not json")];
            assert!(construct_tool_config(&request).unwrap().is_none());
        }
    }
}
