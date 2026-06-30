use eventsource_stream::{EventStreamError, Eventsource};
use futures::{StreamExt, stream};
use log::{error, warn};
use serde_json::Value;

use crate::{
    ProviderId,
    constants::{ENVIRONMENT_CONTEXT, INSTRUCTION},
    provider::{
        ChatEvent, ChatRequest, ChatStream, ConversationItem, Model, ProviderError, Result,
        codec::{ClaudeCodec, EncodeMode, ProviderDecoder, ProviderEncoder},
        runtime::{AvailableModels, MODELS_CACHE_TTL_SECS, SSE_IDLE_TIMEOUT, unix_now},
    },
};

pub(super) const RESPONSES_URL: &str = "https://api.anthropic.com/v1/messages";
pub(super) const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const EFFORT_ORDER: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L3349-L3358
/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L3041-L3331
/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1776-L1799
/// https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/messages/messages.ts#L1276-L1287
pub(super) fn build_request_body(request: &ChatRequest) -> Value {
    let mut tools: Vec<Value> = request
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect();

    tools.push(serde_json::json!({
      "type": "web_search_20250305",
      "name": "web_search",
    }));

    let messages: Vec<Value> = request
        .messages
        .iter()
        .filter_map(|e| {
            let mode = if ProviderId::ClaudeCode == e.provider_id
                || ProviderId::Anthropic == e.provider_id
            {
                EncodeMode::SameProviderReplay
            } else {
                EncodeMode::CrossProvider
            };
            ClaudeCodec.encode_conversation_item(&e.payload, mode)
        })
        .collect();

    let system_prompt = vec![
        serde_json::json!({
            "type": "text",
            "text": INSTRUCTION
        }),
        ClaudeCodec.encode_env_context(&ENVIRONMENT_CONTEXT),
    ];

    serde_json::json!({
        "max_tokens": 16000,
        "model": request.model,
        "system": system_prompt,
        "messages": messages,
        "stream": true,
        "tools": tools,
        "thinking": {
            "type": "adaptive"
        },
        "output_config": {
            "effort": request.effort
        }
    })
}

#[derive(Debug)]
enum BlockState {
    Supported(Value),
    Ignored,
}

pub(super) fn response_event_stream(
    response: reqwest::Response,
    provider_id: ProviderId,
) -> ChatStream {
    let sse = response.bytes_stream().eventsource();
    let stream = stream::unfold(Some((sse, None)), move |state| async move {
        let (mut sse, mut placeholder) = state?;
        loop {
            let next = match tokio::time::timeout(SSE_IDLE_TIMEOUT, sse.next()).await {
                Err(_) => {
                    return Some((
                        Err(ProviderError::Transport(format!(
                            "SSE idle timeout: no activity for {SSE_IDLE_TIMEOUT:?}"
                        ))),
                        None,
                    ));
                },
                Ok(None) => return None,
                Ok(Some(frame)) => frame,
            };

            match next {
                Err(EventStreamError::Transport(e)) => {
                    return Some((
                        Err(ProviderError::Transport(format!(
                            "SSE transport error: {e}"
                        ))),
                        None,
                    ));
                },
                Err(e) => {
                    return Some((
                        Err(ProviderError::Other(format!("SSE parse error: {e}"))),
                        None,
                    ));
                },
                // https://docs.claude.com/en/docs/build-with-claude/streaming
                Ok(frame) => match frame.event.as_str() {
                    "message_start" => continue,
                    "content_block_start" => {
                        if placeholder.is_some() {
                            error!(
                                "unexpected placeholder value exists. This indicates a bug. {:?}",
                                placeholder
                            );
                            return Some((
                                Err(ProviderError::Other(
                                    "invalid state during response parsing".into(),
                                )),
                                None,
                            ));
                        }

                        match serde_json::from_str(&frame.data)
                            .map_err(ProviderError::from)
                            .and_then(|v| parse_content_start_event(&v))
                        {
                            Ok(item) => {
                                placeholder = Some(item);
                                continue;
                            },
                            Err(e) => return Some((Err(e), None)),
                        }
                    },
                    "content_block_delta" => {
                        return match serde_json::from_str(&frame.data)
                            .map_err(ProviderError::from)
                            .and_then(|v| parse_content_delta(&v, provider_id, &mut placeholder))
                        {
                            Ok(Some(event)) => Some((Ok(event), Some((sse, placeholder)))),
                            Ok(None) => continue,
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "content_block_stop" => {
                        return match finalize_block_content(placeholder.take()) {
                            Ok(Some(item)) => {
                                Some((Ok(ChatEvent::OutputItem { item }), Some((sse, None))))
                            },
                            Ok(None) => continue,
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "message_delta" => continue,
                    "message_stop" => return Some((Ok(ChatEvent::Done), None)),
                    "ping" => continue,
                    "error" => return Some((Err(parse_stream_error(&frame.data)), None)),
                    event => {
                        warn!(
                            "{provider_id:?} messages SSE: unknown event type {event:?}; skipping."
                        );
                        continue;
                    },
                },
            }
        }
    });

    Box::pin(stream)
}

fn parse_content_start_event(data: &Value) -> Result<BlockState> {
    let mut content = data
        .get("content_block")
        .ok_or_else(|| ProviderError::Other("missing content block".into()))?
        .clone();

    let content_type = content
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing type field in content".into()))?;

    if !ClaudeCodec::SUPPORTED_RESPONSE_TYPES.contains(&content_type) {
        warn!("unsupported response type {}", content_type);
        return Ok(BlockState::Ignored);
    }

    // Proactively add the citations field in text to ease the burden.
    if content_type == "text"
        && let Some(obj) = content.as_object_mut()
    {
        obj.entry("citations")
            .or_insert_with(|| Value::Array(Vec::new()));
    }

    Ok(BlockState::Supported(content))
}

/// https://platform.claude.com/docs/en/build-with-claude/citations#streaming-support
fn parse_content_delta(
    data: &Value,
    provider_id: ProviderId,
    placeholder: &mut Option<BlockState>,
) -> Result<Option<ChatEvent>> {
    let placeholder = match placeholder {
        Some(BlockState::Ignored) => return Ok(None),
        Some(BlockState::Supported(value)) => value,
        None => {
            error!("no placeholder value exists. This indicates a bug.");
            return Err(ProviderError::Other(
                "invalid state during response parsing".into(),
            ));
        },
    };

    let content = data
        .get("delta")
        .ok_or_else(|| ProviderError::Other("missing delta block".into()))?;

    let content_type = content
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Other("missing type field in content".into()))?;

    match content_type {
        "text_delta" => {
            append_delta_field(placeholder, content, "text", "text")?;
            ClaudeCodec
                .decode_output_text_delta(content.clone())
                .map(|text| Some(ChatEvent::TextDelta { provider_id, text }))
        },
        "thinking_delta" => {
            append_delta_field(placeholder, content, "thinking", "thinking")?;
            ClaudeCodec
                .decode_reasoning_delta(content.clone())
                .map(|text| Some(ChatEvent::ReasoningSummaryDelta { text }))
        },
        "signature_delta" => {
            append_delta_field(placeholder, content, "signature", "signature")?;
            Ok(None)
        },
        "input_json_delta" => {
            merge_partial_json(placeholder, content)?;
            Ok(None)
        },
        "citations_delta" => {
            append_delta_field(placeholder, content, "citations", "citation")?;
            Ok(None)
        },
        _ => {
            warn!("unknown delta type {}.", content_type);
            Ok(None)
        },
    }
}

fn append_delta_field(
    acc: &mut Value,
    next: &Value,
    target_field: &str,
    source_field: &str,
) -> Result<()> {
    let source = match next.get(source_field) {
        Some(source) => source,
        None => {
            error!("missing source field {source_field} in delta json {next:?}");
            return Err(ProviderError::Other(format!(
                "missing source field {source_field}"
            )));
        },
    };
    let target = match acc.get_mut(target_field) {
        Some(target) => target,
        None => {
            error!("missing target field {target_field} in accumulated json {acc:?}");
            return Err(ProviderError::Other(format!(
                "missing target field {target_field}"
            )));
        },
    };

    match (target, source) {
        (Value::String(target), Value::String(source)) => target.push_str(source),
        (Value::Array(target), Value::Array(source)) => target.extend(source.iter().cloned()),
        (Value::Array(target), Value::Object(_)) => target.push(source.clone()),
        (target, source) => {
            error!(
                "unexpected delta field types for {target_field}: target={target:?}, source={source:?}"
            );
            return Err(ProviderError::Other(format!(
                "invalid delta field type for {target_field}"
            )));
        },
    }

    Ok(())
}

fn merge_partial_json(acc: &mut Value, next: &Value) -> Result<()> {
    let Some(input) = acc.get_mut("input") else {
        error!("missing input block from accumulate json {:?}", acc);
        return Err(ProviderError::Other("missing input block".into()));
    };

    let partial_json = next
        .get("partial_json")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error!("missing partial json field in delta json {:?}", next);
            ProviderError::Other("missing partial json field in delta".into())
        })?;

    match input {
        Value::String(input) => input.push_str(partial_json),
        Value::Object(map) if map.is_empty() => {
            *input = Value::String(partial_json.to_string());
        },
        other => {
            error!(
                "unexpected input block for partial json accumulation {:?}",
                other
            );
            return Err(ProviderError::Other(
                "invalid input block for partial json accumulation".into(),
            ));
        },
    }

    Ok(())
}

fn finalize_block_content(placeholder: Option<BlockState>) -> Result<Option<ConversationItem>> {
    match placeholder {
        Some(BlockState::Supported(value)) => ClaudeCodec.decode_output_item(value).map(Some),
        Some(BlockState::Ignored) => Ok(None),
        None => {
            error!("no placeholder value exists. This indicates a bug.");
            Err(ProviderError::Other(
                "invalid state during response parsing".into(),
            ))
        },
    }
}

/// https://platform.claude.com/docs/en/build-with-claude/streaming#error-events
pub(super) fn parse_stream_error(data: &str) -> ProviderError {
    let message = serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("claude messages stream error: {data}"));
    ProviderError::Other(message)
}

pub(super) async fn fetch_models(
    request: &reqwest::Client,
    api_key: &str,
) -> Result<AvailableModels> {
    let response = request
        .get(MODELS_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;
    if response.status().is_success() {
        let payload: Value = response.json().await?;
        Ok(AvailableModels {
            models: parse_model_response(payload),
            expires_at: unix_now() + MODELS_CACHE_TTL_SECS,
        })
    } else {
        let error = response.text().await?;
        Err(parse_stream_error(&error))
    }
}

fn parse_model_response(data: Value) -> Vec<Model> {
    data.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            // filter all models do not support effort
            if !item
                .pointer("/capabilities/effort/supported")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }

            // filter all models do not support adaptive thinking
            if !item
                .pointer("/capabilities/thinking/types/adaptive/supported")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }

            let id = item.get("id").and_then(Value::as_str)?;
            let name = item.get("display_name").and_then(Value::as_str)?;
            let effort = item.pointer("/capabilities/effort")?;
            let supported_reasoning_efforts = EFFORT_ORDER
                .iter()
                .filter(|effort_name| {
                    effort
                        .pointer(&format!("/{effort_name}/supported"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .map(|effort| effort.to_string())
                .collect::<Vec<_>>();
            let default_reasoning_effort = supported_reasoning_efforts
                .get(1)
                .or_else(|| supported_reasoning_efforts.first())
                .cloned()?;

            Some(Model {
                id: id.to_string(),
                name: name.to_string(),
                default_reasoning_effort,
                supported_reasoning_efforts,
            })
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod parse_model_response_tests {
    use super::*;

    fn payload() -> Value {
        serde_json::from_str(
            r#"{
              "data": [
                {
                  "type": "model",
                  "id": "claude-fable-5",
                  "display_name": "Claude Fable 5",
                  "created_at": "2026-06-07T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 128000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": true }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": true },
                      "max": { "supported": true }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": false },
                        "adaptive": { "supported": true }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-opus-4-8",
                  "display_name": "Claude Opus 4.8",
                  "created_at": "2026-05-28T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 128000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": true }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": true },
                      "max": { "supported": true }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": false },
                        "adaptive": { "supported": true }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-opus-4-7",
                  "display_name": "Claude Opus 4.7",
                  "created_at": "2026-04-14T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 128000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": true }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": true },
                      "max": { "supported": true }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": false },
                        "adaptive": { "supported": true }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-sonnet-4-6",
                  "display_name": "Claude Sonnet 4.6",
                  "created_at": "2026-02-17T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 128000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": true }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": false },
                      "max": { "supported": true }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": true }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-opus-4-6",
                  "display_name": "Claude Opus 4.6",
                  "created_at": "2026-02-04T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 128000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": true }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": false },
                      "max": { "supported": true }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": true }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-opus-4-5-20251101",
                  "display_name": "Claude Opus 4.5",
                  "created_at": "2025-11-24T00:00:00Z",
                  "max_input_tokens": 200000,
                  "max_tokens": 64000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": false }
                    },
                    "effort": {
                      "supported": true,
                      "low": { "supported": true },
                      "medium": { "supported": true },
                      "high": { "supported": true },
                      "xhigh": { "supported": false },
                      "max": { "supported": false }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": false }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-haiku-4-5-20251001",
                  "display_name": "Claude Haiku 4.5",
                  "created_at": "2025-10-15T00:00:00Z",
                  "max_input_tokens": 200000,
                  "max_tokens": 64000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": false },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": false }
                    },
                    "effort": {
                      "supported": false,
                      "low": { "supported": false },
                      "medium": { "supported": false },
                      "high": { "supported": false },
                      "xhigh": { "supported": false },
                      "max": { "supported": false }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": false }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-sonnet-4-5-20250929",
                  "display_name": "Claude Sonnet 4.5",
                  "created_at": "2025-09-29T00:00:00Z",
                  "max_input_tokens": 1000000,
                  "max_tokens": 64000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": true },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": false }
                    },
                    "effort": {
                      "supported": false,
                      "low": { "supported": false },
                      "medium": { "supported": false },
                      "high": { "supported": false },
                      "xhigh": { "supported": false },
                      "max": { "supported": false }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": false }
                      }
                    }
                  }
                },
                {
                  "type": "model",
                  "id": "claude-opus-4-1-20250805",
                  "display_name": "Claude Opus 4.1",
                  "created_at": "2025-08-05T00:00:00Z",
                  "max_input_tokens": 200000,
                  "max_tokens": 32000,
                  "capabilities": {
                    "batch": { "supported": true },
                    "citations": { "supported": true },
                    "code_execution": { "supported": false },
                    "context_management": {
                      "supported": true,
                      "clear_tool_uses_20250919": { "supported": true },
                      "clear_thinking_20251015": { "supported": true },
                      "compact_20260112": { "supported": false }
                    },
                    "effort": {
                      "supported": false,
                      "low": { "supported": false },
                      "medium": { "supported": false },
                      "high": { "supported": false },
                      "xhigh": { "supported": false },
                      "max": { "supported": false }
                    },
                    "image_input": { "supported": true },
                    "pdf_input": { "supported": true },
                    "structured_outputs": { "supported": true },
                    "thinking": {
                      "supported": true,
                      "types": {
                        "enabled": { "supported": true },
                        "adaptive": { "supported": false }
                      }
                    }
                  }
                }
              ],
              "has_more": false,
              "first_id": "claude-fable-5",
              "last_id": "claude-opus-4-1-20250805"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_models_with_effort_and_adaptive_thinking_support() {
        let models = parse_model_response(payload());
        let ids = models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "claude-fable-5",
                "claude-opus-4-8",
                "claude-opus-4-7",
                "claude-sonnet-4-6",
                "claude-opus-4-6",
            ]
        );
        assert_eq!(models[0].name, "Claude Fable 5");
    }

    #[test]
    fn parses_supported_efforts_from_capability_object() {
        let models = parse_model_response(payload());
        let sonnet = models
            .iter()
            .find(|model| model.id == "claude-sonnet-4-6")
            .unwrap();
        let mut efforts = sonnet.supported_reasoning_efforts.clone();

        efforts.sort();

        assert_eq!(efforts, vec!["high", "low", "max", "medium"]);
        assert!(
            !sonnet
                .supported_reasoning_efforts
                .iter()
                .any(|effort| effort == "xhigh")
        );
    }

    #[test]
    fn uses_medium_supported_effort_as_default() {
        let models = parse_model_response(payload());

        for model in models {
            assert_eq!(model.default_reasoning_effort, "medium");
        }
    }

    #[test]
    fn uses_first_supported_effort_as_default_when_only_one_exists() {
        let models = parse_model_response(serde_json::json!({
            "data": [
                {
                    "id": "claude-test",
                    "display_name": "Claude Test",
                    "capabilities": {
                        "effort": {
                            "supported": true,
                            "low": { "supported": true },
                            "medium": { "supported": false }
                        },
                        "thinking": {
                            "types": {
                                "adaptive": { "supported": true }
                            }
                        }
                    }
                }
            ]
        }));

        assert_eq!(
            models,
            vec![Model {
                id: "claude-test".to_string(),
                name: "Claude Test".to_string(),
                default_reasoning_effort: "low".to_string(),
                supported_reasoning_efforts: vec!["low".to_string()],
            }]
        );
    }
}

#[cfg(test)]
mod parse_content_delta_tests {
    use super::*;

    fn json(data: &str) -> Value {
        serde_json::from_str(data).unwrap()
    }

    fn placeholder(data: &str) -> Option<BlockState> {
        Some(parse_content_start_event(&json(data)).unwrap())
    }

    fn apply_delta(placeholder: &mut Option<BlockState>, data: &str) -> Option<ChatEvent> {
        parse_content_delta(&json(data), ProviderId::Anthropic, placeholder).unwrap()
    }

    fn block_value(placeholder: Option<BlockState>) -> Value {
        match placeholder {
            Some(BlockState::Supported(value)) => value,
            other => panic!("expected supported block state, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_content_block_is_ignored_until_stop() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"unknown_block","text":""}}"#,
        );
        assert!(matches!(placeholder, Some(BlockState::Ignored)));

        let event = apply_delta(
            &mut placeholder,
            &serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": "ignored"
                }
            })
            .to_string(),
        );

        assert!(event.is_none());
        assert!(finalize_block_content(placeholder).unwrap().is_none());
    }

    #[test]
    fn delta_without_placeholder_is_fatal() {
        let mut placeholder = None;

        let error = parse_content_delta(
            &serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "unknown_delta",
                    "value": "ignored"
                }
            }),
            ProviderId::Anthropic,
            &mut placeholder,
        )
        .unwrap_err();

        assert!(matches!(error, ProviderError::Other(_)));
    }

    /// https://platform.claude.com/docs/en/build-with-claude/streaming#basic-streaming-request
    #[test]
    fn text_delta_accumulates_text_and_returns_delta_event() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        );
        let chunks = [
            "Okay",
            ",",
            " let",
            "'s",
            " check",
            " the",
            " weather",
            " for",
            " San",
            " Francisco",
            ",",
            " CA",
            ":",
        ];

        let mut rendered = String::new();
        for chunk in chunks {
            let event = apply_delta(
                &mut placeholder,
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "text_delta",
                        "text": chunk
                    }
                })
                .to_string(),
            );
            let Some(ChatEvent::TextDelta { provider_id, text }) = event else {
                panic!("expected text delta event");
            };
            assert_eq!(provider_id, ProviderId::Anthropic);
            rendered.push_str(&text);
        }

        let block = block_value(placeholder);
        assert_eq!(
            rendered,
            "Okay, let's check the weather for San Francisco, CA:"
        );
        assert_eq!(
            block,
            serde_json::json!({
                "type": "text",
                "text": "Okay, let's check the weather for San Francisco, CA:",
                "citations": []
            })
        );
    }

    /// https://platform.claude.com/docs/en/build-with-claude/citations#streaming-support
    #[test]
    fn citations_delta_accumulates_single_citation_on_text_block() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        );

        let text_event = apply_delta(
            &mut placeholder,
            &serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": "the grass is green"
                }
            })
            .to_string(),
        );
        let Some(ChatEvent::TextDelta { text, .. }) = text_event else {
            panic!("expected text delta event");
        };
        assert_eq!(text, "the grass is green");

        let citation = serde_json::json!({
            "type": "char_location",
            "cited_text": "The grass is green.",
            "document_index": 0,
            "document_title": "Example Document",
            "start_char_index": 0,
            "end_char_index": 20
        });
        let citation_event = apply_delta(
            &mut placeholder,
            &serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "citations_delta",
                    "citation": citation
                }
            })
            .to_string(),
        );

        assert!(citation_event.is_none());
        assert_eq!(
            block_value(placeholder),
            serde_json::json!({
                "type": "text",
                "text": "the grass is green",
                "citations": [
                    {
                        "type": "char_location",
                        "cited_text": "The grass is green.",
                        "document_index": 0,
                        "document_title": "Example Document",
                        "start_char_index": 0,
                        "end_char_index": 20
                    }
                ]
            })
        );
    }

    /// https://platform.claude.com/docs/en/build-with-claude/streaming#streaming-request-with-extended-thinking
    #[test]
    fn thinking_and_signature_delta_accumulate_thinking_block() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
        );
        let chunks = ["I need", " to check", " the weather"];

        let mut rendered = String::new();
        for chunk in chunks {
            let event = apply_delta(
                &mut placeholder,
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "thinking_delta",
                        "thinking": chunk
                    }
                })
                .to_string(),
            );
            let Some(ChatEvent::ReasoningSummaryDelta { text }) = event else {
                panic!("expected reasoning summary delta event");
            };
            rendered.push_str(&text);
        }

        let event = apply_delta(
            &mut placeholder,
            &serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "signature_delta",
                    "signature": "example_signature"
                }
            })
            .to_string(),
        );
        assert!(event.is_none());

        let block = block_value(placeholder);
        assert_eq!(rendered, "I need to check the weather");
        assert_eq!(
            block,
            serde_json::json!({
                "type": "thinking",
                "thinking": "I need to check the weather",
                "signature": "example_signature"
            })
        );
    }

    /// https://platform.claude.com/docs/en/build-with-claude/streaming#streaming-request-with-tool-use
    #[test]
    fn input_json_delta_accumulates_tool_use_partial_json() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01T1x1fJ34qAmk2tNTrN7Up6","name":"get_weather","input":{}}}"#,
        );
        let chunks = [
            "",
            r#"{"location":"#,
            r#" "San"#,
            " Francisc",
            "o,",
            r#" CA"}"#,
        ];

        for chunk in chunks {
            let event = apply_delta(
                &mut placeholder,
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": chunk
                    }
                })
                .to_string(),
            );
            assert!(event.is_none());
        }

        let block = block_value(placeholder);
        assert_eq!(
            block,
            serde_json::json!({
                "type": "tool_use",
                "id": "toolu_01T1x1fJ34qAmk2tNTrN7Up6",
                "name": "get_weather",
                "input": r#"{"location": "San Francisco, CA"}"#
            })
        );
    }

    #[test]
    fn input_json_delta_accumulates_server_tool_use_partial_json() {
        let mut placeholder = placeholder(
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srvtoolu_014hJH82Qum7Td6UV8gDXThB","name":"web_search","input":{}}}"#,
        );
        let chunks = [
            "",
            r#"{"query"#,
            r#"":"#,
            r#" "weather"#,
            " NY",
            "C to",
            r#"day"}"#,
        ];

        for chunk in chunks {
            let event = apply_delta(
                &mut placeholder,
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": chunk
                    }
                })
                .to_string(),
            );
            assert!(event.is_none());
        }

        let block = block_value(placeholder);
        assert_eq!(
            block,
            serde_json::json!({
                "type": "server_tool_use",
                "id": "srvtoolu_014hJH82Qum7Td6UV8gDXThB",
                "name": "web_search",
                "input": r#"{"query": "weather NYC today"}"#
            })
        );
    }
}

#[cfg(test)]
mod parse_stream_error_tests {
    use super::*;

    fn message(data: &str) -> String {
        match parse_stream_error(data) {
            ProviderError::Other(message) => message,
            other => panic!("expected ProviderError::Other, got {other:?}"),
        }
    }

    #[test]
    fn stream_error_extracts_nested_error_message() {
        let data = r#"{
          "type": "error",
          "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
          }
        }"#;

        assert_eq!(message(data), "Overloaded");
    }

    #[test]
    fn malformed_json_falls_back_to_raw_payload() {
        assert_eq!(
            message("not json at all"),
            "claude messages stream error: not json at all"
        );
    }
}
