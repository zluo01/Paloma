use std::time::Duration;

use eventsource_stream::{EventStreamError, Eventsource};
use futures::StreamExt;
use log::warn;
use scry_provider_base::{
    Dispatcher, ENVIRONMENT_CONTEXT, ProviderDecoder, ProviderEncoder, ProviderError,
    SSE_IDLE_TIMEOUT,
};
use scry_provider_protocol::v1::{ChatRequest, Done, EncodeMode, Model, TextDelta, chat_response};
use serde::Deserialize;
use serde_json::Value;

use crate::{constant::PROVIDER_ID, runtime::codec::CodexCodec};

pub(super) const MODELS_FETCH_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn build_request_body(request: &ChatRequest, backend_id: String) -> Value {
    // Wrap each backend-agnostic ToolSchema in the OpenAI Responses API
    // function-tool envelope. `strict: false` matches Codex CLI's behaviour
    // — strict mode requires the schema to be exhaustively closed (every
    // object marks `additionalProperties: false`), which schemars-generated
    // schemas don't guarantee.
    let mut tools: Vec<Value> = request
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
            Some(serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "strict": false,
                "parameters": parameters,
            }))
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
        let mode = if PROVIDER_ID == e.provider_id && backend_id == e.backend_id {
            EncodeMode::SameProvider
        } else {
            EncodeMode::CrossProvider
        };
        CodexCodec.encode_conversation_item(e.item.as_ref()?, mode)
    }));

    serde_json::json!({
        "model": request.model,
        "instructions": request.instruction,
        "input": messages,
        "stream": true,
        "store": false,
        "tools": tools,
        "reasoning": { "effort": request.effort, "summary": "auto" },
        "include": ["reasoning.encrypted_content"],
    })
}

pub(super) async fn response_event_stream(
    response: reqwest::Response,
    backend_id: String,
    dispatcher: Dispatcher,
) {
    let mut sse = response.bytes_stream().eventsource();
    let mut reasoning_summary_delta_seen = false;
    loop {
        let next = match tokio::time::timeout(SSE_IDLE_TIMEOUT, sse.next()).await {
            Err(_) => {
                dispatcher
                    .send_chat_event(chat_response::Payload::Error(format!(
                        "SSE idle timeout: no activity for {SSE_IDLE_TIMEOUT:?}"
                    )))
                    .await;
                return;
            },
            Ok(None) => {
                dispatcher
                    .send_chat_event(chat_response::Payload::Error(
                        "SSE stream ended without response.completed".into(),
                    ))
                    .await;
                return;
            },
            Ok(Some(frame)) => frame,
        };

        match next {
            Err(EventStreamError::Transport(e)) => {
                dispatcher
                    .send_chat_event(chat_response::Payload::Error(format!(
                        "SSE transport error: {e}"
                    )))
                    .await;
                return;
            },
            Err(e) => {
                dispatcher
                    .send_chat_event(chat_response::Payload::Error(format!(
                        "SSE parse error: {e}"
                    )))
                    .await;
                return;
            },
            Ok(frame) => match frame.event.as_str() {
                "response.output_text.delta" => {
                    match serde_json::from_str(&frame.data)
                        .map_err(ProviderError::from)
                        .and_then(|payload| CodexCodec.decode_output_text_delta(payload))
                    {
                        Ok(delta) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::TextDelta(TextDelta {
                                    provider_id: PROVIDER_ID.into(),
                                    backend_id: backend_id.clone(),
                                    delta,
                                }))
                                .await;
                        },
                        Err(e) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::Error(e.to_string()))
                                .await;
                            return;
                        },
                    };
                },
                "response.reasoning_summary_text.delta" => {
                    match serde_json::from_str(&frame.data)
                        .map_err(ProviderError::from)
                        .and_then(|payload| CodexCodec.decode_reasoning_delta(payload))
                    {
                        Ok(text) => {
                            reasoning_summary_delta_seen = true;
                            dispatcher
                                .send_chat_event(chat_response::Payload::ReasoningDelta(text))
                                .await;
                        },
                        Err(e) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::Error(e.to_string()))
                                .await;
                            return;
                        },
                    };
                },
                "response.reasoning_summary_part.added" => {
                    reasoning_summary_delta_seen = false;
                    dispatcher
                        .send_chat_event(chat_response::Payload::ReasoningDelta(String::new()))
                        .await;
                },
                "response.reasoning_summary_text.done" => {
                    if reasoning_summary_delta_seen {
                        continue;
                    }
                    match serde_json::from_str(&frame.data)
                        .map_err(ProviderError::from)
                        .and_then(|payload| CodexCodec.decode_reasoning_delta(payload))
                    {
                        Ok(text) => {
                            reasoning_summary_delta_seen = true;
                            dispatcher
                                .send_chat_event(chat_response::Payload::ReasoningDelta(text))
                                .await;
                        },
                        Err(e) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::Error(e.to_string()))
                                .await;
                            return;
                        },
                    };
                },
                "response.output_item.done" => {
                    match serde_json::from_str(&frame.data)
                        .map_err(ProviderError::from)
                        .and_then(|payload| CodexCodec.decode_output_item(payload))
                    {
                        Ok(item) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::OutputItem(item))
                                .await;
                        },
                        Err(e) => {
                            dispatcher
                                .send_chat_event(chat_response::Payload::Error(e.to_string()))
                                .await;
                            return;
                        },
                    };
                },
                "response.completed" => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Done(Done {}))
                        .await;
                    return;
                },
                "response.failed" | "response.incomplete" | "error" => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(
                            parse_stream_error(&frame.data).to_string(),
                        ))
                        .await;
                    return;
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
                    warn!("{backend_id:?} responses SSE: unknown event type {event:?}; skipping.");
                    continue;
                },
            },
        }
    }
}

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.failed
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.incomplete
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#error
/// https://developers.openai.com/api/reference/resources/realtime/server-events#error
pub(super) fn parse_stream_error(data: &str) -> ProviderError {
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
                .or_else(|| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
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
        .filter_map(|m| {
            let supported_reasoning_efforts: Vec<String> = m
                .supported_reasoning_levels
                .into_iter()
                .map(|p| p.effort)
                .collect();

            // only keep models with reasoning efforts
            let default_reasoning_effort = m
                .default_reasoning_level
                .filter(|level| supported_reasoning_efforts.contains(level))
                .or_else(|| supported_reasoning_efforts.first().cloned())?;

            Some(Model {
                id: m.slug,
                name: m.display_name,
                default_reasoning_effort,
                supported_reasoning_efforts,
            })
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
mod models_from_response_tests {
    use scry_provider_protocol::v1::Model;

    use super::*;

    fn models(raw: &str) -> Vec<Model> {
        let response: ModelsResponse = serde_json::from_str(raw).expect("valid models response");
        models_from_response(response)
    }

    #[test]
    fn model_without_reasoning_efforts_is_dropped() {
        let parsed = models(
            r#"{"models": [{
              "slug": "chat-only",
              "display_name": "Chat Only",
              "visibility": "list",
              "supported_in_api": true,
              "priority": 1,
              "default_reasoning_level": "medium",
              "supported_reasoning_levels": []
            }]}"#,
        );
        assert!(parsed.is_empty());
    }

    #[test]
    fn default_effort_is_always_one_of_the_supported_efforts() {
        let parsed = models(
            r#"{"models": [{
              "slug": "reasoner",
              "display_name": "Reasoner",
              "visibility": "list",
              "supported_in_api": true,
              "priority": 1,
              "default_reasoning_level": "medium",
              "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}]
            }]}"#,
        );
        let model = parsed.first().expect("model kept");
        assert_eq!(model.default_reasoning_effort, "low");
        assert!(
            model
                .supported_reasoning_efforts
                .contains(&model.default_reasoning_effort)
        );
    }

    #[test]
    fn supported_default_effort_is_preserved() {
        let parsed = models(
            r#"{"models": [{
              "slug": "reasoner",
              "display_name": "Reasoner",
              "visibility": "list",
              "supported_in_api": true,
              "priority": 1,
              "default_reasoning_level": "high",
              "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}]
            }]}"#,
        );
        assert_eq!(parsed[0].default_reasoning_effort, "high");
    }
}

#[cfg(test)]
mod parse_stream_error_tests {
    use scry_provider_base::ProviderError;

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
    fn error_event_extracts_nested_error_message() {
        let data = r#"{
          "type": "error",
          "error": {
            "type": "insufficient_quota",
            "code": "insufficient_quota",
            "message": "You exceeded your current quota, please check your plan and billing details.",
            "param": null
          },
          "sequence_number": 2
        }"#;
        assert_eq!(
            message(data),
            "You exceeded your current quota, please check your plan and billing details."
        );
    }

    #[test]
    fn unparseable_payload_falls_back_to_raw_dump() {
        assert_eq!(message("not json"), "response failed: not json");
    }
}

#[cfg(test)]
mod build_request_body_tests {
    use scry_provider_protocol::v1::{
        ChatRequestMessage, ConversationItem, ConversationMessage, MessageContentItem, Reasoning,
        SummaryItem, conversation_item::Item,
    };

    use super::*;
    use crate::constant::backend_id;

    fn request() -> ChatRequest {
        ChatRequest {
            session_id: "session".into(),
            instruction: "example instruction".into(),
            model: "gpt-5".into(),
            effort: "medium".into(),
            messages: vec![],
            tools: vec![],
        }
    }

    fn reasoning_message(provider: &str, backend: &str) -> ChatRequestMessage {
        ChatRequestMessage {
            provider_id: provider.into(),
            backend_id: backend.into(),
            item: Some(ConversationItem {
                item: Some(Item::Reasoning(Reasoning {
                    reasoning: vec![SummaryItem {
                        content: "ABC".into(),
                        provider_meta: Default::default(),
                    }],
                    provider_meta: [("A".to_string(), serde_json::json!("B").to_string())].into(),
                })),
            }),
        }
    }

    fn text_message(provider: &str, backend: &str) -> ChatRequestMessage {
        ChatRequestMessage {
            provider_id: provider.into(),
            backend_id: backend.into(),
            item: Some(ConversationItem {
                item: Some(Item::Message(ConversationMessage {
                    message: vec![MessageContentItem {
                        content: "ABC".into(),
                        provider_meta: Default::default(),
                    }],
                    provider_meta: [("A".to_string(), serde_json::json!("B").to_string())].into(),
                })),
            }),
        }
    }

    #[test]
    fn same_provider_and_backend_replays_metadata() {
        let mut request = request();
        request.messages = vec![
            reasoning_message(PROVIDER_ID, backend_id::CODEX),
            text_message(PROVIDER_ID, backend_id::CODEX),
        ];

        let body = build_request_body(&request, backend_id::CODEX.into());
        let input = body.get("input").and_then(Value::as_array).unwrap();

        // input[0] is the environment context.
        assert_eq!(input.len(), 3);
        assert_eq!(
            input[1],
            serde_json::json!({
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": "ABC" }],
                "A": "B"
            })
        );
        assert_eq!(
            input[2],
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "ABC" }],
                "A": "B"
            })
        );
    }

    #[test]
    fn same_provider_different_backend_drops_reasoning_and_metadata() {
        let mut request = request();
        request.messages = vec![
            reasoning_message(PROVIDER_ID, backend_id::OPENAI_API),
            text_message(PROVIDER_ID, backend_id::OPENAI_API),
        ];

        let body = build_request_body(&request, backend_id::CODEX.into());
        let input = body.get("input").and_then(Value::as_array).unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(
            input[1],
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "ABC" }]
            })
        );
    }

    #[test]
    fn foreign_provider_with_same_backend_name_drops_reasoning_and_metadata() {
        let mut request = request();
        request.messages = vec![
            reasoning_message("Anthropic", backend_id::CODEX),
            text_message("Anthropic", backend_id::CODEX),
        ];

        let body = build_request_body(&request, backend_id::CODEX.into());
        let input = body.get("input").and_then(Value::as_array).unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(
            input[1],
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "ABC" }]
            })
        );
    }
}
