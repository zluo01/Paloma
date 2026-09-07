use aws_sdk_bedrockruntime::types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStart, ContentBlockStartEvent,
    ConverseStreamOutput, ReasoningContentBlockDelta,
};
use aws_smithy_types::base64;
use log::{error, warn};
use paloma_provider_base::{ProviderError, Result, provider_meta};
use paloma_provider_protocol::v1::{
    ConversationItem, ConversationMessage, MessageContentItem, Reasoning, SummaryItem, TextDelta,
    ToolCall, Unknown, chat_response, conversation_item::Item,
};
use serde_json::json;

use super::codec::{META_REDACTED_CONTENT, META_SIGNATURE};
use crate::constant::{PROVIDER_ID, backend_id};

#[derive(Debug)]
pub(super) enum BlockState {
    Text(String),
    Reasoning {
        text: String,
        signature: String,
        redacted: Option<Vec<u8>>,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
}

pub(super) enum StreamStep {
    Continue(Option<chat_response::Payload>),
    Item(ConversationItem),
    Done,
}

pub(super) fn on_stream_event(
    event: &ConverseStreamOutput,
    placeholder: &mut Option<BlockState>,
) -> Result<StreamStep> {
    match event {
        ConverseStreamOutput::MessageStart(_) | ConverseStreamOutput::Metadata(_) => {
            Ok(StreamStep::Continue(None))
        },
        ConverseStreamOutput::ContentBlockStart(start) => {
            parse_block_start_event(start, placeholder).map(|()| StreamStep::Continue(None))
        },
        ConverseStreamOutput::ContentBlockDelta(delta) => {
            parse_block_delta_event(delta, placeholder).map(StreamStep::Continue)
        },
        ConverseStreamOutput::ContentBlockStop(_) => {
            Ok(match finalize_block_content(placeholder.take()) {
                Some(item) => StreamStep::Item(item),
                None => StreamStep::Continue(None),
            })
        },
        ConverseStreamOutput::MessageStop(_) => Ok(StreamStep::Done),
        other => {
            warn!("unknown converse stream event {other:?}; skipping.");
            Ok(StreamStep::Continue(None))
        },
    }
}

fn parse_block_start_event(
    event: &ContentBlockStartEvent,
    placeholder: &mut Option<BlockState>,
) -> Result<()> {
    if placeholder.is_some() {
        error!("unexpected placeholder value exists. This indicates a bug. {placeholder:?}");
        return Err(ProviderError::Other(
            "invalid state during response parsing".into(),
        ));
    }

    match event.start() {
        Some(ContentBlockStart::ToolUse(start)) => {
            *placeholder = Some(BlockState::ToolUse {
                id: start.tool_use_id().to_string(),
                name: start.name().to_string(),
                input: String::new(),
            });
        },
        Some(other) => warn!("unsupported content block start {other:?}; skipping."),
        None => warn!("content block start without payload; skipping."),
    }
    Ok(())
}

fn parse_block_delta_event(
    event: &ContentBlockDeltaEvent,
    placeholder: &mut Option<BlockState>,
) -> Result<Option<chat_response::Payload>> {
    let Some(delta) = event.delta() else {
        warn!("content block delta without payload; skipping.");
        return Ok(None);
    };

    match delta {
        ContentBlockDelta::Text(chunk) => {
            match placeholder {
                Some(BlockState::Text(text)) => text.push_str(chunk),
                None => *placeholder = Some(BlockState::Text(chunk.clone())),
                Some(other) => return Err(mismatched_delta("text", other)),
            }
            Ok(Some(chat_response::Payload::TextDelta(TextDelta {
                provider_id: PROVIDER_ID.into(),
                backend_id: backend_id::BEDROCK_API.into(),
                delta: chunk.clone(),
            })))
        },
        ContentBlockDelta::ReasoningContent(reasoning) => {
            let state = match placeholder {
                Some(BlockState::Reasoning { .. }) => placeholder.as_mut().unwrap(),
                None => {
                    *placeholder = Some(BlockState::Reasoning {
                        text: String::new(),
                        signature: String::new(),
                        redacted: None,
                    });
                    placeholder.as_mut().unwrap()
                },
                Some(other) => return Err(mismatched_delta("reasoning", other)),
            };
            let BlockState::Reasoning {
                text,
                signature,
                redacted,
            } = state
            else {
                unreachable!("state is narrowed to reasoning above");
            };

            match reasoning {
                ReasoningContentBlockDelta::Text(chunk) => {
                    text.push_str(chunk);
                    Ok(Some(chat_response::Payload::ReasoningDelta(chunk.clone())))
                },
                ReasoningContentBlockDelta::Signature(chunk) => {
                    signature.push_str(chunk);
                    Ok(None)
                },
                ReasoningContentBlockDelta::RedactedContent(blob) => {
                    redacted
                        .get_or_insert_with(Vec::new)
                        .extend_from_slice(blob.as_ref());
                    Ok(None)
                },
                other => {
                    warn!("unknown reasoning delta {other:?}; skipping.");
                    Ok(None)
                },
            }
        },
        ContentBlockDelta::ToolUse(tool) => match placeholder {
            Some(BlockState::ToolUse { input, .. }) => {
                input.push_str(tool.input());
                Ok(None)
            },
            other => Err(ProviderError::Other(format!(
                "tool use delta without a tool use block start: {other:?}"
            ))),
        },
        other => {
            warn!("unknown content block delta {other:?}; skipping.");
            Ok(None)
        },
    }
}

fn finalize_block_content(placeholder: Option<BlockState>) -> Option<ConversationItem> {
    let item = match placeholder? {
        BlockState::Text(text) => Item::Message(ConversationMessage {
            message: vec![MessageContentItem {
                content: text,
                provider_meta: Default::default(),
            }],
            provider_meta: Default::default(),
        }),
        BlockState::Reasoning {
            redacted: Some(redacted),
            ..
        } => Item::Unknown(Unknown {
            provider_meta: provider_meta(
                &json!({ META_REDACTED_CONTENT: base64::encode(redacted) }),
                &[],
            ),
        }),
        BlockState::Reasoning {
            text, signature, ..
        } => {
            let meta = if signature.is_empty() {
                json!({})
            } else {
                json!({ META_SIGNATURE: signature })
            };
            Item::Reasoning(Reasoning {
                reasoning: vec![SummaryItem {
                    content: text,
                    provider_meta: Default::default(),
                }],
                provider_meta: provider_meta(&meta, &[]),
            })
        },
        BlockState::ToolUse { id, name, input } => Item::ToolCall(ToolCall {
            call_id: id,
            name,
            arguments: input,
            provider_meta: Default::default(),
        }),
    };
    Some(ConversationItem { item: Some(item) })
}

fn mismatched_delta(delta_kind: &str, state: &BlockState) -> ProviderError {
    error!(
        "{delta_kind} delta arrived for mismatched block state {state:?}. This indicates a bug."
    );
    ProviderError::Other("invalid state during response parsing".into())
}

#[cfg(test)]
mod tests {
    use aws_sdk_bedrockruntime::types::{ToolUseBlockDelta, ToolUseBlockStart};

    use super::*;

    fn text_delta(chunk: &str) -> ContentBlockDeltaEvent {
        ContentBlockDeltaEvent::builder()
            .delta(ContentBlockDelta::Text(chunk.into()))
            .content_block_index(0)
            .build()
            .unwrap()
    }

    fn reasoning_delta(delta: ReasoningContentBlockDelta) -> ContentBlockDeltaEvent {
        ContentBlockDeltaEvent::builder()
            .delta(ContentBlockDelta::ReasoningContent(delta))
            .content_block_index(0)
            .build()
            .unwrap()
    }

    #[test]
    fn text_deltas_accumulate_and_emit_delta_events() {
        let mut placeholder = None;
        let mut rendered = String::new();

        for chunk in ["Hello", ", ", "world"] {
            let payload = parse_block_delta_event(&text_delta(chunk), &mut placeholder)
                .unwrap()
                .unwrap();
            let chat_response::Payload::TextDelta(delta) = payload else {
                panic!("expected text delta payload");
            };
            assert_eq!(delta.provider_id, PROVIDER_ID);
            rendered.push_str(&delta.delta);
        }

        assert_eq!(rendered, "Hello, world");
        let item = finalize_block_content(placeholder).unwrap();
        let Some(Item::Message(message)) = item.item else {
            panic!("expected message item");
        };
        assert_eq!(message.message[0].content, "Hello, world");
    }

    #[test]
    fn reasoning_deltas_accumulate_text_and_signature() {
        let mut placeholder = None;

        for chunk in ["I need", " to think"] {
            let payload = parse_block_delta_event(
                &reasoning_delta(ReasoningContentBlockDelta::Text(chunk.into())),
                &mut placeholder,
            )
            .unwrap()
            .unwrap();
            assert!(matches!(
                payload,
                chat_response::Payload::ReasoningDelta(delta) if delta == chunk
            ));
        }
        let no_event = parse_block_delta_event(
            &reasoning_delta(ReasoningContentBlockDelta::Signature("sig".into())),
            &mut placeholder,
        )
        .unwrap();
        assert!(no_event.is_none());

        let item = finalize_block_content(placeholder).unwrap();
        let Some(Item::Reasoning(reasoning)) = item.item else {
            panic!("expected reasoning item");
        };
        assert_eq!(reasoning.reasoning[0].content, "I need to think");
        // provider_meta values are JSON text, like the anthropic codec
        assert_eq!(
            reasoning.provider_meta.get(META_SIGNATURE),
            Some(&"\"sig\"".to_string())
        );
    }

    #[test]
    fn tool_use_accumulates_from_start_through_input_deltas() {
        let mut placeholder = None;

        let start = ContentBlockStartEvent::builder()
            .start(ContentBlockStart::ToolUse(
                ToolUseBlockStart::builder()
                    .tool_use_id("call-1")
                    .name("lookup")
                    .build()
                    .unwrap(),
            ))
            .content_block_index(1)
            .build()
            .unwrap();
        parse_block_start_event(&start, &mut placeholder).unwrap();

        for chunk in [r#"{"q":"#, r#""x"}"#] {
            let event = ContentBlockDeltaEvent::builder()
                .delta(ContentBlockDelta::ToolUse(
                    ToolUseBlockDelta::builder().input(chunk).build().unwrap(),
                ))
                .content_block_index(1)
                .build()
                .unwrap();
            assert!(
                parse_block_delta_event(&event, &mut placeholder)
                    .unwrap()
                    .is_none()
            );
        }

        let item = finalize_block_content(placeholder).unwrap();
        let Some(Item::ToolCall(call)) = item.item else {
            panic!("expected tool call item");
        };
        assert_eq!(call.call_id, "call-1");
        assert_eq!(call.name, "lookup");
        assert_eq!(call.arguments, r#"{"q":"x"}"#);
    }

    #[test]
    fn tool_use_delta_without_start_is_fatal() {
        let mut placeholder = None;
        let event = ContentBlockDeltaEvent::builder()
            .delta(ContentBlockDelta::ToolUse(
                ToolUseBlockDelta::builder().input("{}").build().unwrap(),
            ))
            .content_block_index(0)
            .build()
            .unwrap();

        assert!(parse_block_delta_event(&event, &mut placeholder).is_err());
    }

    #[test]
    fn block_stop_without_accumulated_block_produces_nothing() {
        assert!(finalize_block_content(None).is_none());
    }
}
