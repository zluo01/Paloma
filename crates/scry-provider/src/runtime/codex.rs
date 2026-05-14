use crate::entity::{
    ChatEvent, ChatRequest, ChatStream, Model, ProviderClient, ProviderError, ProviderId,
};
use crate::{Auth, Result};
use base64::Engine;
use eventsource_stream::{EventStreamError, Eventsource};
use futures::stream;
use futures::StreamExt;
use scry_storage::db::Storage;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, RwLock};
use std::time::Duration;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// Codex CLI's own version, sent as `?client_version=` on `/backend-api/codex/models`.
/// The backend gates per-model availability on this via `minimal_client_version`
/// in each `ModelInfo`. Since we're impersonating Codex CLI, this must track
/// what real Codex CLI publishes, not our own package version.
///
/// Construction in Codex (major.minor.patch from `CARGO_PKG_VERSION`):
///   <https://github.com/openai/codex/blob/main/codex-rs/models-manager/src/lib.rs#L19-L26>
/// Query-param append:
///   <https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/models.rs#L35-L38>
/// Current published version (the source of truth — bump to match):
///   <https://www.npmjs.com/package/@openai/codex>
const CLIENT_VERSION: &str = "0.130.0";

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

pub struct CodexRuntime {
    request: reqwest::Client,
    refresh_token: Arc<RwLock<String>>,
    tokens: Arc<RwLock<RefreshTokens>>,
}

impl CodexRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client) -> Result<Self> {
        let refresh_token = match credential {
            Auth::OAuth {
                refresh_token: Some(rt),
                ..
            } => rt.clone(),
            Auth::OAuth {
                refresh_token: None,
                ..
            } => {
                return Err(ProviderError::Other(
                    "Codex credential is missing a refresh_token".into(),
                ));
            }
            Auth::ApiKey(_) => {
                return Err(ProviderError::Other(
                    "Codex does not support api_key credentials".into(),
                ));
            }
        };

        Ok(Self {
            request,
            refresh_token: Arc::new(RwLock::new(refresh_token)),
            tokens: Arc::new(RwLock::new(RefreshTokens {
                access_token: String::new(),
                chatgpt_account_id: String::new(),
            })),
        })
    }
}

async fn fetch_access_token(
    request: &reqwest::Client,
    refresh_token: &str,
) -> Result<(RefreshTokens, String, i64)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let chatgpt_account_id = extract_chatgpt_account_id(&response.id_token)
        .ok_or_else(|| ProviderError::Other("id_token missing chatgpt_account_id claim".into()))?;

    Ok((
        RefreshTokens {
            access_token: response.access_token,
            chatgpt_account_id,
        },
        response.refresh_token,
        response.expires_in,
    ))
}

/// Decode a JWT's middle segment (claims) and pull out
/// `https://api.openai.com/auth.chatgpt_account_id`.
///
/// No signature verification: we received this token over our own TLS exchange
/// with `auth.openai.com`, so the bytes can't have been tampered with in transit.
fn extract_chatgpt_account_id(id_token: &str) -> Option<String> {
    let payload_b64 = id_token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .map(str::to_string)
}

#[async_trait::async_trait]
impl ProviderClient for CodexRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn refresh(&self, storage: &Storage) -> Result<Option<Auth>> {
        let current_refresh_token = self.refresh_token.read().unwrap().clone();
        let (new_tokens, new_refresh_token, expires_in) =
            fetch_access_token(&self.request, &current_refresh_token).await?;
        *self.tokens.write().unwrap() = new_tokens;
        *self.refresh_token.write().unwrap() = new_refresh_token.clone();

        // codex refresh token follows "rotate on use"
        // so we need to proactively update the db whenever we refresh.
        storage
            .update_provider(ProviderId::Codex.as_str(), "oauth", &new_refresh_token)
            .await?;

        Ok(Some(Auth::OAuth {
            refresh_token: Some(new_refresh_token),
            expires_in: Some(expires_in),
        }))
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let (access_token, account_id) = {
            let guard = self.tokens.read().unwrap();
            (guard.access_token.clone(), guard.chatgpt_account_id.clone())
        };

        let body = build_request_body(&request);

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&access_token)
            .header("chatgpt-account-id", &account_id)
            .header("originator", "srcy")
            .header(reqwest::header::USER_AGENT, "scry-codex")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let sse = response.bytes_stream().eventsource();
        let stream = stream::unfold(Some(sse), move |state| async move {
            let mut sse = state?;
            loop {
                let next = match tokio::time::timeout(SSE_IDLE_TIMEOUT, sse.next()).await {
                    Err(_) => {
                        return Some((
                            Err(ProviderError::Transport(format!(
                                "SSE idle timeout: no activity for {SSE_IDLE_TIMEOUT:?}"
                            ))),
                            None,
                        ));
                    }
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
                    }
                    Err(e) => {
                        return Some((
                            Err(ProviderError::Other(format!("SSE parse error: {e}"))),
                            None,
                        ));
                    }
                    // https://developers.openai.com/api/reference/resources/responses/streaming-events
                    //
                    // Event-handling philosophy (mirrors Codex CLI's
                    // `process_responses_event` in `codex-rs/codex-api/src/sse/responses.rs`):
                    //
                    //   1. Stream `*.delta` frames the UI actually renders
                    //      (output text, reasoning summary).
                    //   2. Capture canonical structured state from
                    //      `response.output_item.done` (reasoning items today,
                    //      tool calls / messages later) — this is the single
                    //      event that carries the final, replay-ready payload
                    //      for each output item.
                    //   3. Treat lifecycle as binary: `created` is implicit,
                    //      `completed` ends the turn, `failed`/`incomplete`/`error`
                    //      surface as `ProviderError`.
                    //   4. Drop everything else (status pings, per-content-part
                    //      `.done` events, hosted-tool sub-lifecycles, audio,
                    //      MCP, etc.). They're either redundant with
                    //      `output_item.done` or features we don't expose yet.
                    Ok(frame) => match frame.event.as_str() {
                        // ── Text streaming (renderable) ────────────────────────
                        "response.output_text.delta" => {
                            return match serde_json::from_str::<TextDeltaPayload>(&frame.data) {
                                Ok(p) => {
                                    Some((Ok(ChatEvent::TextDelta { text: p.delta }), Some(sse)))
                                }
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        }
                        "response.reasoning_summary_text.delta" => {
                            return match serde_json::from_str::<TextDeltaPayload>(&frame.data) {
                                Ok(p) => Some((
                                    Ok(ChatEvent::ReasoningSummaryDelta { text: p.delta }),
                                    Some(sse),
                                )),
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        }
                        "response.output_item.done" => {
                            return match serde_json::from_str::<OutputItemDonePayload>(&frame.data)
                            {
                                Ok(p) => {
                                    Some((Ok(ChatEvent::OutputItem { item: p.item }), Some(sse)))
                                }
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        }
                        "response.completed" => return Some((Ok(ChatEvent::Done), None)),
                        "response.failed" | "response.incomplete" | "error" => {
                            return Some((Err(parse_stream_error(&frame.data)), None));
                        }
                        // ── Lifecycle: status pings (no payload we need) ───────
                        // `response.created`     — turn started (we use the HTTP
                        //                          200 itself as the start signal).
                        // `response.in_progress` — heartbeat-ish progress ping.
                        // `response.queued`      — waiting in the request queue.
                        "response.created" | "response.in_progress" | "response.queued" => continue,
                        // ── Output-item skeleton + content-part lifecycle ──────
                        // `output_item.added`        — empty shell of a new item;
                        //                              the filled-in version arrives
                        //                              at `output_item.done`.
                        // `content_part.added/done`  — sub-structure inside a
                        //                              message item; the final
                        //                              content is in `output_item.done`.
                        // `output_text.done`         — finalized text for one
                        //                              content part; same data is
                        //                              inside `output_item.done`.
                        // `output_text.annotation.added` — citations/file refs,
                        //                              also delivered via
                        //                              `output_item.done`.
                        "response.output_item.added"
                        | "response.content_part.added"
                        | "response.content_part.done"
                        | "response.output_text.done"
                        | "response.output_text.annotation.added" => continue,
                        // ── Refusal stream (not exposed as a ChatEvent yet) ────
                        // When the model refuses, a refusal content part is
                        // streamed in parallel with — or instead of — output
                        // text. We currently surface refusals as part of the
                        // assistant message text (via `output_item.done`); these
                        // per-delta frames would only matter for live rendering.
                        "response.refusal.delta" | "response.refusal.done" => continue,
                        // ── Reasoning sub-lifecycle (non-renderable parts) ─────
                        // `reasoning_summary_part.added/done` — summary section
                        //   boundaries; UI uses them to start a new "Thinking…"
                        //   block. We don't yet expose section breaks.
                        // `reasoning_summary_text.done` — finalized summary;
                        //   already accumulated from `.delta`.
                        // `reasoning_text.delta/done` — raw (unencrypted) CoT,
                        //   only emitted when reasoning is *not* encrypted.
                        //   We always request `include: ["reasoning.encrypted_content"]`
                        //   so these are not expected in practice.
                        "response.reasoning_summary_part.added"
                        | "response.reasoning_summary_part.done"
                        | "response.reasoning_summary_text.done"
                        | "response.reasoning_text.delta"
                        | "response.reasoning_text.done" => continue,
                        // ── Tool-call argument streaming ───────────────────────
                        // For function calls Codex deliberately waits for the
                        // typed item at `output_item.done` rather than streaming
                        // partial JSON (validation cost + half-parsed args are
                        // useless). Custom-tool input frames are renderable in
                        // principle (e.g. live patch preview) but we don't have
                        // a ChatEvent variant for them yet.
                        "response.function_call_arguments.delta"
                        | "response.function_call_arguments.done"
                        | "response.custom_tool_call_input.delta"
                        | "response.custom_tool_call_input.done" => continue,
                        // ── Hosted tools: all sub-lifecycle events ignored ─────
                        // For every hosted tool, the only durable record is the
                        // resolved call delivered via `output_item.done`. The
                        // `.in_progress` / `.searching` / `.generating` /
                        // `.interpreting` pings just drive spinners we don't
                        // render. None of these tools are enabled in
                        // `build_request_body` (`"tools": []`) so we shouldn't
                        // see them today — listed for completeness so future
                        // tool enablement only needs to flip the arm.
                        "response.file_search_call.in_progress"
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
                        | "response.mcp_list_tools.failed" => continue,
                        // ── Audio output (no audio surface yet) ────────────────
                        "response.audio.delta"
                        | "response.audio.done"
                        | "response.audio.transcript.delta"
                        | "response.audio.transcript.done" => continue,
                        // ── Unknown / future events ────────────────────────────
                        // The Responses API evolves: new sub-lifecycle frames,
                        // new tool families, etc. Treat unknown events the same
                        // way Codex does — log nothing, keep polling, let the
                        // turn finish on `response.completed`.
                        _ => continue,
                    },
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<Model>> {
        let (access_token, account_id) = {
            let guard = self.tokens.read().unwrap();
            (guard.access_token.clone(), guard.chatgpt_account_id.clone())
        };

        let url = format!("{MODELS_URL}?client_version={CLIENT_VERSION}");
        let response: ModelsResponse = self
            .request
            .get(&url)
            .bearer_auth(&access_token)
            .header("chatgpt-account-id", &account_id)
            .header("OpenAI-Beta", "responses=experimental")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

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

        Ok(models
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
            .collect())
    }

    fn construct_user_prompt(&self, prompt: String) -> Value {
        serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": prompt }
            ]
        })
    }
}

/// Tokens derived from a refresh-token exchange. Held together under one lock
/// so a refresh rotates `access_token` (and re-derives `chatgpt_account_id`)
/// atomically.
struct RefreshTokens {
    access_token: String,
    chatgpt_account_id: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_in: i64,
    refresh_token: String,
    access_token: String,
    id_token: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
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

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.reasoning_summary_text.delta
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_text.delta
#[derive(Debug, Deserialize)]
struct TextDeltaPayload {
    delta: String,
}

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_item.done
#[derive(Debug, Deserialize)]
struct OutputItemDonePayload {
    item: Value,
}

fn build_request_body(request: &ChatRequest) -> Value {
    serde_json::json!({
        "model": request.model,
        "instructions": "",
        "input": request.messages,
        "stream": true,
        "store": false,
        "tools": [],
        "reasoning": { "effort": request.effort, "summary": "auto" },
        "include": ["reasoning.encrypted_content"],
    })
}

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.failed
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.incomplete
/// https://developers.openai.com/api/reference/resources/responses/streaming-events#error
fn parse_stream_error(data: &str) -> ProviderError {
    let msg = serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("response failed: {data}"));
    ProviderError::Other(msg)
}
