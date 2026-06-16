use std::{
    sync::{
        RwLock,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::Engine;
use eventsource_stream::{EventStreamError, Eventsource};
use futures::{StreamExt, stream};
use log::error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{
    config::INSTRUCTION,
    db::{AuthKind, ProviderId, Storage},
    provider::{
        Auth, ChatEvent, ChatRequest, ChatStream, Model, ProviderClient, ProviderError,
        ProviderHealthStatus, Result,
    },
};

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
const CLIENT_VERSION: &str = "0.139.0";

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a fetched model catalogue is served from cache before a refetch.
const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(1).as_secs();

pub struct CodexRuntime {
    request: reqwest::Client,
    storage: Storage,
    refresh_token: RwLock<Auth>,
    tokens: RwLock<AccessToken>,
    status: AtomicU8,
    error: RwLock<Option<String>>,
    models: Mutex<Option<AvailableModels>>,
}

impl CodexRuntime {
    pub async fn new(credential: &Auth, request: reqwest::Client, storage: Storage) -> Self {
        let auth = match credential {
            Auth::OAuth {
                refresh_token: Some(_),
                ..
            } => credential.clone(),
            Auth::OAuth {
                refresh_token: None,
                ..
            } => {
                return Self::unhealthy(
                    request,
                    storage,
                    "Codex credential is missing a refresh_token".into(),
                );
            },
            Auth::ApiKey(_) => {
                return Self::unhealthy(
                    request,
                    storage,
                    "Codex does not support api_key credentials".into(),
                );
            },
        };

        match fetch_access_token(&request, &auth, &storage).await {
            Ok((access_token, auth)) => match fetch_models(&request, &access_token).await {
                Ok(models) => Self {
                    request,
                    refresh_token: RwLock::new(auth),
                    tokens: RwLock::new(access_token),
                    storage,
                    status: AtomicU8::new(ProviderHealthStatus::Running as u8),
                    error: RwLock::new(None),
                    models: Mutex::new(Some(models)),
                },
                Err(e) => {
                    error!("fail to fetch models on initialization. {e}");
                    Self::unhealthy(request, storage, format!("fail to connect to codex: {e}"))
                },
            },
            Err(e) => {
                error!("fail to refresh for access token on initialization. {e}");
                Self::unhealthy(request, storage, format!("fail to connect to codex: {e}"))
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(request: reqwest::Client, storage: Storage, error_msg: String) -> Self {
        Self {
            request,
            refresh_token: RwLock::new(Auth::OAuth {
                refresh_token: None,
                expires_at: None,
            }),
            tokens: RwLock::new(AccessToken {
                access_token: String::new(),
                chatgpt_account_id: String::new(),
            }),
            storage,
            status: AtomicU8::new(ProviderHealthStatus::Unhealthy as u8),
            error: RwLock::new(Some(error_msg)),
            models: Mutex::new(None),
        }
    }

    /// Flag the runtime unhealthy and record `error` for status reporting.
    fn mark_unhealthy(&self, error: String) {
        self.status
            .store(ProviderHealthStatus::Unhealthy as u8, Ordering::Relaxed);
        *self.error.write().unwrap() = Some(error);
    }

    /// refresh access token if the current token is close to expired or already expired,
    /// otherwise return the current access token
    async fn refresh(&self) -> Result<AccessToken> {
        let valid_until = match *self.refresh_token.read().unwrap() {
            Auth::OAuth {
                expires_at: Some(expires_at),
                ..
            // 5-minute margin before the token actually expires.
            } => expires_at.saturating_sub(Duration::from_mins(5).as_secs()),
            _ => 0,
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Still valid: hand back the cached token untouched.
        if now <= valid_until {
            return Ok(self.tokens.read().unwrap().clone());
        }

        // Expired (or about to): rotate the token and cache the new pair.
        let current_refresh_token = self.refresh_token.read().unwrap().clone();
        let (new_tokens, auth) =
            fetch_access_token(&self.request, &current_refresh_token, &self.storage).await?;
        *self.refresh_token.write().unwrap() = auth;
        *self.tokens.write().unwrap() = new_tokens.clone();
        Ok(new_tokens)
    }
}

#[async_trait::async_trait]
impl ProviderClient for CodexRuntime {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream> {
        let token = self
            .refresh()
            .await
            .inspect_err(|e| self.mark_unhealthy(e.to_string()))?;

        let body = build_request_body(&request);

        let response = self
            .request
            .post(RESPONSES_URL)
            .bearer_auth(&token.access_token)
            .header("chatgpt-account-id", &token.chatgpt_account_id)
            .header("originator", "srcy")
            .header(reqwest::header::USER_AGENT, "scry-codex")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

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
                                Ok(p) => Some((
                                    Ok(ChatEvent::TextDelta { text: p.delta }),
                                    Some((sse, reasoning_summary_delta_seen)),
                                )),
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        },
                        "response.reasoning_summary_text.delta" => {
                            return match serde_json::from_str::<TextDeltaPayload>(&frame.data) {
                                Ok(p) => {
                                    reasoning_summary_delta_seen = true;
                                    Some((
                                        Ok(ChatEvent::ReasoningSummaryDelta { text: p.delta }),
                                        Some((sse, reasoning_summary_delta_seen)),
                                    ))
                                },
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
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
                            return match serde_json::from_str::<TextDonePayload>(&frame.data) {
                                Ok(p) => Some((
                                    Ok(ChatEvent::ReasoningSummaryDelta { text: p.text }),
                                    Some((sse, true)),
                                )),
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        },
                        "response.output_item.done" => {
                            return match serde_json::from_str::<OutputItemDonePayload>(&frame.data)
                            {
                                Ok(p) => {
                                    let event = if p.item.get("type").and_then(|t| t.as_str())
                                        == Some("function_call")
                                    {
                                        ChatEvent::ToolCallItem { item: p.item }
                                    } else {
                                        ChatEvent::OutputItem { item: p.item }
                                    };
                                    Some((Ok(event), Some((sse, reasoning_summary_delta_seen))))
                                },
                                Err(e) => Some((Err(ProviderError::from(e)), None)),
                            };
                        },
                        "response.completed" => return Some((Ok(ChatEvent::Done), None)),
                        "response.failed" | "response.incomplete" | "error" => {
                            return Some((Err(parse_stream_error(&frame.data)), None));
                        },
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
                        "response.reasoning_summary_part.done"
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
                        // `.interpreting` pings just drive UI spinners; we
                        // don't render per-tool affordances yet, so skipping
                        // is fine — the final call still flows through the
                        // generic `OutputItem` handler above.
                        //
                        // Status note: `web_search` IS enabled in
                        // `build_request_body`, so `response.web_search_call.*`
                        // frames fire on real turns and are intentionally
                        // skipped here. The other hosted-tool arms are dormant
                        // (their tools are not advertised) and listed only so
                        // enabling any of them later is a one-line change.
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
                        // new tool families, etc. Surface them at error level
                        // so a new event type is visible in logs (add an arm
                        // when one shows up), then keep polling — the turn
                        // still resolves on `response.completed`.
                        event => {
                            log::debug!(
                                "codex SSE: unknown event type {event:?}; skipping. \
                                 If this is a new official event, add it to the match \
                                 (see https://developers.openai.com/api/reference/resources/responses/streaming-events)."
                            );
                            continue;
                        },
                    },
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Option<Vec<Model>> {
        // Hold the lock across the refetch: concurrent callers single-flight
        // — the first refetches, the rest wait and pick up its cached result.
        let mut cache = self.models.lock().await;

        // Serve the cached catalogue while it's still fresh.
        if let Some(cached) = cache.as_ref() {
            if unix_now() < cached.expires_at {
                return Some(cached.models.clone());
            }
        }

        match self.refresh().await {
            Ok(access_token) => match fetch_models(&self.request, &access_token).await {
                Ok(result) => {
                    let models = result.models.clone();
                    *cache = Some(result);
                    Some(models)
                },
                Err(e) => {
                    error!("failed to refresh model catalogue: {e}");
                    cache.as_ref().map(|cached| cached.models.clone())
                },
            },
            Err(e) => {
                error!("failed to get access token to refresh model catalogue: {e}");
                self.mark_unhealthy(e.to_string());
                cache.as_ref().map(|cached| cached.models.clone())
            },
        }
    }

    fn health_statue(&self) -> ProviderHealthStatus {
        ProviderHealthStatus::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn error(&self) -> Option<String> {
        self.error.read().unwrap().clone()
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

    fn construct_function_call_output(&self, call_id: String, output: String) -> Value {
        serde_json::json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        })
    }
}

async fn fetch_access_token(
    request: &reqwest::Client,
    auth: &Auth,
    storage: &Storage,
) -> Result<(AccessToken, Auth)> {
    let response: RefreshResponse = request
        .post(TOKEN_URL)
        .json(&RefreshRequest::new(auth)?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let chatgpt_account_id = extract_chatgpt_account_id(&response.id_token)
        .ok_or_else(|| ProviderError::Other("id_token missing chatgpt_account_id claim".into()))?;

    // codex refresh token follows "rotate on use"
    // so we need to proactively update the db whenever we refresh.
    storage
        .update_provider(
            &ProviderId::Codex,
            &AuthKind::Oauth,
            &response.refresh_token,
        )
        .await?;

    // get the epoch time current access token will expire
    let expires_at = unix_now() + response.expires_in;

    Ok((
        AccessToken {
            access_token: response.access_token,
            chatgpt_account_id,
        },
        Auth::OAuth {
            refresh_token: Some(response.refresh_token),
            expires_at: Some(expires_at),
        },
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

async fn fetch_models(request: &reqwest::Client, token: &AccessToken) -> Result<AvailableModels> {
    let url = format!("{MODELS_URL}?client_version={CLIENT_VERSION}");
    let response: ModelsResponse = request
        .get(&url)
        .bearer_auth(&token.access_token)
        .header("chatgpt-account-id", &token.chatgpt_account_id)
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

    let available_models = models
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
        .collect();
    Ok(AvailableModels {
        models: available_models,
        expires_at: unix_now() + MODELS_CACHE_TTL_SECS,
    })
}

struct AvailableModels {
    models: Vec<Model>,
    expires_at: u64,
}

/// unix epoch in seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Tokens derived from a refresh-token exchange. Held together under one lock
/// so a refresh rotates `access_token` (and re-derives `chatgpt_account_id`)
/// atomically.
#[derive(Clone)]
struct AccessToken {
    access_token: String,
    chatgpt_account_id: String,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

impl<'a> RefreshRequest<'a> {
    /// Build the refresh-grant body from a stored OAuth credential, failing
    /// if it carries no refresh token.
    fn new(auth: &'a Auth) -> Result<Self> {
        let Auth::OAuth {
            refresh_token: Some(refresh_token),
            ..
        } = auth
        else {
            return Err(ProviderError::Other("missing a refresh_token".into()));
        };
        Ok(Self {
            client_id: CLIENT_ID,
            grant_type: "refresh_token",
            refresh_token,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    /// Seconds until `access_token` expires (e.g. `863999` ≈ 10 days).
    expires_in: u64,
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

#[derive(Debug, Deserialize)]
struct TextDonePayload {
    text: String,
}

/// https://developers.openai.com/api/reference/resources/responses/streaming-events#response.output_item.done
#[derive(Debug, Deserialize)]
struct OutputItemDonePayload {
    item: Value,
}

fn build_request_body(request: &ChatRequest) -> Value {
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

    serde_json::json!({
        "model": request.model,
        "instructions": INSTRUCTION,
        "input": request.messages,
        "stream": true,
        "store": false,
        "tools": tools,
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
