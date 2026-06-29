use std::{sync::LazyLock, time::Duration};

use eventsource_stream::{EventStreamError, Eventsource};
use futures::{StreamExt, stream};
use log::warn;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    constants::{ENVIRONMENT_CONTEXT, INSTRUCTION},
    entity::ProviderId,
    provider::{
        ChatEvent, ChatRequest, ChatStream, Model, ProviderError,
        codec::{CodexCodec, EncodeMode, ProviderDecoder, ProviderEncoder},
    },
};

/// How long a fetched model catalogue is served from cache before a refetch.
pub(super) const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(1).as_secs();

/// https://github.com/openai/codex/blob/main/codex-rs/models-manager/models.json
pub(super) static OPENAI_MODEL_CATALOG: LazyLock<Vec<Model>> = LazyLock::new(|| {
    let response = serde_json::from_str(include_str!("models.json"))
        .expect("bundled OpenAI model catalog should parse");
    models_from_response(response)
});

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn build_request_body(request: &ChatRequest, provider_id: ProviderId) -> Value {
    // Wrap each provider-agnostic ToolSchema in the OpenAI Responses API
    // function-tool envelope. `strict: false` matches Codex CLI's behaviour
    // — strict mode requires the schema to be exhaustively closed (every
    // object marks `additionalProperties: false`), which schemars-generated
    // schemas don't guarantee.
    let mut tools: Vec<Value> = request
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "strict": false,
                "parameters": t.parameters,
            })
        })
        .collect();

    // Append the hosted `web_search` tool. The Responses API runs the
    // search server-side (no shell/curl scraping needed) and emits the
    // finalized call as a `web_search_call` item on the `output_item.done`
    // SSE frame, which our generic `OutputItem` handler persists so later
    // turns can reference the results. `external_web_access: true` selects
    // the live (non-cached) variant — matches Codex CLI's
    // `WebSearchMode::Live`. See
    // <https://platform.openai.com/docs/guides/tools-web-search>.
    tools.push(serde_json::json!({
        "type": "web_search",
        "external_web_access": true,
    }));

    let mut messages: Vec<Value> = vec![CodexCodec.encode_env_context(&ENVIRONMENT_CONTEXT)];
    messages.extend(request.messages.iter().filter_map(|e| {
        let mode = if provider_id == e.provider_id {
            EncodeMode::SameProviderReplay
        } else {
            EncodeMode::CrossProvider
        };
        CodexCodec.encode_conversation_item(&e.payload, mode)
    }));

    serde_json::json!({
        "model": request.model,
        "instructions": INSTRUCTION,
        "input": messages,
        "stream": true,
        "store": false,
        "tools": tools,
        "reasoning": { "effort": request.effort, "summary": "auto" },
        "include": ["reasoning.encrypted_content"],
    })
}

pub(super) fn response_event_stream(
    response: reqwest::Response,
    provider_id: ProviderId,
) -> ChatStream {
    let sse = response.bytes_stream().eventsource();
    let stream = stream::unfold(Some((sse, false)), move |state| async move {
        let (mut sse, mut reasoning_summary_delta_seen) = state?;
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
                Ok(frame) => match frame.event.as_str() {
                    "response.output_text.delta" => {
                        return match CodexCodec.decode_output_text_delta(&frame.data) {
                            Ok(text) => Some((
                                Ok(ChatEvent::TextDelta { provider_id, text }),
                                Some((sse, reasoning_summary_delta_seen)),
                            )),
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "response.reasoning_summary_text.delta" => {
                        return match CodexCodec.decode_reasoning_delta(&frame.data) {
                            Ok(text) => {
                                reasoning_summary_delta_seen = true;
                                Some((
                                    Ok(ChatEvent::ReasoningSummaryDelta { text }),
                                    Some((sse, reasoning_summary_delta_seen)),
                                ))
                            },
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "response.reasoning_summary_part.added" => {
                        reasoning_summary_delta_seen = false;
                        return Some((
                            Ok(ChatEvent::ReasoningSummaryDelta {
                                text: String::new(),
                            }),
                            Some((sse, reasoning_summary_delta_seen)),
                        ));
                    },
                    "response.reasoning_summary_text.done" => {
                        if reasoning_summary_delta_seen {
                            continue;
                        }
                        return match CodexCodec.decode_reasoning_delta(&frame.data) {
                            Ok(text) => Some((
                                Ok(ChatEvent::ReasoningSummaryDelta { text }),
                                Some((sse, true)),
                            )),
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "response.output_item.done" => {
                        return match CodexCodec.decode_output_item(&frame.data) {
                            Ok(item) => Some((
                                Ok(ChatEvent::OutputItem { item }),
                                Some((sse, reasoning_summary_delta_seen)),
                            )),
                            Err(e) => Some((Err(e), None)),
                        };
                    },
                    "response.completed" => return Some((Ok(ChatEvent::Done), None)),
                    "response.failed" | "response.incomplete" | "error" => {
                        return Some((Err(parse_stream_error(&frame.data)), None));
                    },
                    "response.created"
                    | "response.in_progress"
                    | "response.queued"
                    | "response.metadata"
                    | "response.output_item.added"
                    | "response.content_part.added"
                    | "response.content_part.done"
                    | "response.output_text.done"
                    | "response.output_text.annotation.added"
                    | "response.refusal.delta"
                    | "response.refusal.done"
                    | "response.reasoning_summary_part.done"
                    | "response.reasoning_text.delta"
                    | "response.reasoning_text.done"
                    | "response.function_call_arguments.delta"
                    | "response.function_call_arguments.done"
                    | "response.custom_tool_call_input.delta"
                    | "response.custom_tool_call_input.done"
                    | "response.file_search_call.in_progress"
                    | "response.file_search_call.searching"
                    | "response.file_search_call.completed"
                    | "response.web_search_call.in_progress"
                    | "response.web_search_call.searching"
                    | "response.web_search_call.completed"
                    | "response.image_generation_call.in_progress"
                    | "response.image_generation_call.generating"
                    | "response.image_generation_call.partial_image"
                    | "response.image_generation_call.completed"
                    | "response.code_interpreter_call.in_progress"
                    | "response.code_interpreter_call.interpreting"
                    | "response.code_interpreter_call.completed"
                    | "response.code_interpreter_call_code.delta"
                    | "response.code_interpreter_call_code.done"
                    | "response.mcp_call.in_progress"
                    | "response.mcp_call.completed"
                    | "response.mcp_call.failed"
                    | "response.mcp_call_arguments.delta"
                    | "response.mcp_call_arguments.done"
                    | "response.mcp_list_tools.in_progress"
                    | "response.mcp_list_tools.completed"
                    | "response.mcp_list_tools.failed"
                    | "response.audio.delta"
                    | "response.audio.done"
                    | "response.audio.transcript.delta"
                    | "response.audio.transcript.done" => continue,
                    event => {
                        warn!(
                            "{provider_id:?} responses SSE: unknown event type {event:?}; skipping."
                        );
                        continue;
                    },
                },
            }
        }
    });

    Box::pin(stream)
}

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.failed
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.incomplete
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#error
fn parse_stream_error(data: &str) -> ProviderError {
    let msg = serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|v| {
            v.pointer("/response/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    v.pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                        .map(|reason| format!("response incomplete: {reason}"))
                })
                .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_string))
        })
        .unwrap_or_else(|| format!("response failed: {data}"));
    ProviderError::Other(msg)
}

pub(super) fn models_from_response(response: ModelsResponse) -> Vec<Model> {
    let mut models: Vec<RawModel> = response
        .models
        .into_iter()
        .filter(|m| m.supported_in_api && m.visibility == "list")
        .collect();
    // Higher priority first; tie-break alphabetically on slug.
    models.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.slug.cmp(&b.slug))
    });

    models
        .into_iter()
        .map(|m| Model {
            id: m.slug,
            name: m.display_name,
            default_reasoning_effort: m.default_reasoning_level.unwrap_or("medium".to_string()),
            supported_reasoning_efforts: m
                .supported_reasoning_levels
                .into_iter()
                .map(|p| p.effort)
                .collect(),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelsResponse {
    models: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    slug: String,
    display_name: String,
    visibility: String,
    supported_in_api: bool,
    priority: i32,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<RawReasoningEffortPreset>,
}

#[derive(Debug, Deserialize)]
struct RawReasoningEffortPreset {
    effort: String,
}

#[cfg(test)]
mod parse_stream_error_tests {
    use super::*;

    fn message(data: &str) -> String {
        let ProviderError::Other(msg) = parse_stream_error(data) else {
            panic!("expected ProviderError::Other");
        };
        msg
    }

    #[test]
    fn response_failed_extracts_nested_error_message() {
        let data = r#"{
          "type": "response.failed",
          "response": {
            "id": "resp_123",
            "object": "response",
            "created_at": 1740855869,
            "status": "failed",
            "completed_at": null,
            "error": {
              "code": "server_error",
              "message": "The model failed to generate a response."
            },
            "incomplete_details": null,
            "model": "gpt-4o-mini-2024-07-18",
            "output": [],
            "metadata": {}
          }
        }"#;
        assert_eq!(message(data), "The model failed to generate a response.");
    }

    #[test]
    fn response_incomplete_extracts_reason() {
        let data = r#"{
          "type": "response.incomplete",
          "response": {
            "id": "resp_123",
            "object": "response",
            "created_at": 1740855869,
            "status": "incomplete",
            "completed_at": null,
            "error": null,
            "incomplete_details": {
              "reason": "max_tokens"
            },
            "model": "gpt-4o-mini-2024-07-18",
            "output": [],
            "metadata": {}
          },
          "sequence_number": 1
        }"#;
        assert_eq!(message(data), "response incomplete: max_tokens");
    }

    #[test]
    fn error_event_extracts_top_level_message() {
        let data = r#"{
          "type": "error",
          "code": "ERR_SOMETHING",
          "message": "Something went wrong",
          "param": null,
          "sequence_number": 1
        }"#;
        assert_eq!(message(data), "Something went wrong");
    }

    #[test]
    fn unparseable_payload_falls_back_to_raw_dump() {
        assert_eq!(message("not json"), "response failed: not json");
    }
}
