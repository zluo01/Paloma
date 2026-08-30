//! UniFFI bindings for `paloma-core`.
//!
//! The core spawns Tokio tasks and drives sockets internally, while UniFFI
//! polls exported futures on foreign (Swift, C#) threads that have no ambient
//! runtime. Every core call is therefore spawned onto the FFI-owned [`RUNTIME`]
//! and only the join handle is awaited on the foreign side; streams are pumped
//! into channels from inside the runtime for the same reason.

// The bridge only exists for the macos and windows frontends; elsewhere the crate
// (and its whole dependency tree, moved to a target section) compiles empty.
#![cfg(any(target_os = "macos", target_os = "windows"))]

mod error;
mod types;

use std::{
    future::Future,
    path::PathBuf,
    pin::pin,
    sync::{Arc, LazyLock, Mutex},
};

use futures::{Stream, StreamExt};
use paloma_core::AppContext;
use tokio::{
    runtime::Runtime,
    sync::{Mutex as AsyncMutex, mpsc},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub use crate::{error::PalomaError, types::*};

uniffi::setup_scaffolding!("paloma");

/// Route `log` records to `<log_path>/Paloma/paloma-YYYY-MM-DD.log`
/// (per-day files, appended) and to stderr for debugger/terminal runs. Call
/// once at app startup, before [`PalomaApp::new`]; extra calls are no-ops.
/// `RUST_LOG` overrides the default filter.
#[uniffi::export]
pub fn init_logging(log_path: String) {
    let env = env_logger::Env::default().default_filter_or("info,rmcp=warn");
    let mut builder = env_logger::Builder::from_env(env);
    builder.format_timestamp_millis();
    if let Some(file) = log_file(log_path) {
        builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
    }
    let _ = builder.try_init();
}

/// Expose only in windows so can do internal plugins initialization in process executable
/// Execute within the core through DLL will cause "deadlock" from loader lock
#[cfg(windows)]
#[uniffi::export]
pub fn process_entry() {
    paloma_core::process_entry();
}

/// The frontend's sink into the same log file; Swift has no portable
/// path into `log`.
#[uniffi::export]
pub fn log_error(target: String, message: String) {
    log::error!(target: target.as_str(), "{message}");
}

fn log_file(log_path: String) -> Option<std::fs::File> {
    let dir = PathBuf::from(log_path).join("Paloma");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("paloma-{}.log", chrono::Local::now().format("%Y-%m-%d"));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
        .ok()
}

struct Tee(std::fs::File);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.0.flush()
    }
}

/// Matches core's render channel so the pump adds no extra slack.
const EVENT_BUFFER: usize = paloma_core::RENDER_CHANNEL_CAPACITY;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("paloma-core")
        .build()
        .expect("failed to start the paloma tokio runtime")
});

/// Run `task` to completion on [`RUNTIME`]; the returned future is safe to
/// poll from any thread.
async fn on_runtime<T, F>(task: F) -> Result<T, PalomaError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    RUNTIME
        .spawn(task)
        .await
        .map_err(|e| PalomaError::new(e.to_string()))
}

/// Like [`on_runtime`], but a fired `token` aborts the task, which surfaces
/// as a "cancelled" failure.
///
/// https://github.com/mozilla/uniffi-rs/issues/2771#issuecomment-3754440908
async fn run_cancellable<T, F>(token: &CancelToken, task: F) -> Result<T, PalomaError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let handle = RUNTIME.spawn(task);
    let abort = handle.abort_handle();
    tokio::select! {
        _ = token.0.cancelled() => {
            abort.abort();
            Err(PalomaError::new("cancelled"))
        }
        outcome = handle => outcome.map_err(|join_error| PalomaError::new(join_error.to_string())),
    }
}

#[derive(uniffi::Object)]
pub struct CancelToken(CancellationToken);

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self(CancellationToken::new()))
    }

    /// Idempotent; a call that already settled is unaffected.
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

/// Pull-based adapter over a core event stream: Swift awaits `next()` until it
/// returns `None`, typically wrapped in an `AsyncStream`.
#[derive(uniffi::Object)]
pub struct EventStream {
    rx: AsyncMutex<mpsc::Receiver<RenderEvent>>,
}

impl EventStream {
    /// Forward `stream` into a channel from a task inside [`RUNTIME`], so the
    /// core stream is always polled with a reactor available. Must be called
    /// from within the runtime.
    fn pump(stream: impl Stream<Item = paloma_core::RenderEvent> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel(EVENT_BUFFER);
        RUNTIME.spawn(async move {
            let mut stream = pin!(stream);
            while let Some(event) = stream.next().await {
                if tx.send(event.into()).await.is_err() {
                    // Swift dropped the stream object; stop draining.
                    break;
                }
            }
        });
        Self {
            rx: AsyncMutex::new(rx),
        }
    }
}

#[uniffi::export]
impl EventStream {
    /// Next event, or `None` once the underlying stream finishes.
    pub async fn next(&self) -> Option<RenderEvent> {
        self.rx.lock().await.recv().await
    }
}

/// A chat turn: the session it belongs to plus its render events.
#[derive(uniffi::Object)]
pub struct ChatStream {
    session_id: Option<Uuid>,
    events: EventStream,
}

#[uniffi::export]
impl ChatStream {
    pub fn session_id(&self) -> Option<Uuid> {
        self.session_id
    }

    /// Next event, or `None` once the turn finishes.
    pub async fn next(&self) -> Option<RenderEvent> {
        self.events.next().await
    }
}

/// Opaque handle to an in-flight MCP OAuth flow. It owns the local callback
/// listener, so it must be handed back to `finalize_mcp_connection` exactly
/// once.
#[derive(uniffi::Object)]
pub struct McpOauthSession {
    inner: Mutex<Option<paloma_core::OAuthCallbackState>>,
    auth_url: String,
}

#[uniffi::export]
impl McpOauthSession {
    /// URL the user must open in a browser to authorize the connection.
    pub fn auth_url(&self) -> String {
        self.auth_url.clone()
    }
}

impl McpOauthSession {
    fn take(&self) -> Result<paloma_core::OAuthCallbackState, PalomaError> {
        self.inner
            .lock()
            .expect("MCP OAuth session lock poisoned")
            .take()
            .ok_or_else(|| PalomaError::new("MCP OAuth session was already consumed"))
    }
}

/// FFI entry point wrapping [`paloma_core::AppContext`].
#[derive(uniffi::Object)]
pub struct PalomaApp {
    inner: Arc<AppContext>,
}

#[uniffi::export]
impl PalomaApp {
    /// Build the whole core: storage, providers, and the plugin/session/turn
    /// event loops, all living on the FFI-owned runtime.
    #[uniffi::constructor]
    pub async fn new(app_data_path: String) -> Result<Arc<Self>, PalomaError> {
        let inner = on_runtime(AppContext::build(PathBuf::from(app_data_path))).await??;
        Ok(Arc::new(Self { inner }))
    }

    /// Run a launcher search; results arrive on the returned stream.
    pub async fn search(&self, input: String) -> Result<Arc<EventStream>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        let stream =
            on_runtime(async move { EventStream::pump(inner.search(&input).await) }).await?;
        Ok(Arc::new(stream))
    }

    /// Run an action off the caller's thread: handlers may spawn processes
    /// or touch the pasteboard, which shouldn't stall the UI thread.
    pub async fn run_search_action(
        &self,
        extension_capability_id: ExtensionCapabilityId,
        action: Action,
    ) -> Result<Behavior, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move {
            inner
                .run_search_action(extension_capability_id, action)
                .await
        })
        .await??
        .into())
    }

    /// Start (or continue) a chat turn. `session_id` of `None` opens a new
    /// session; the created id is reported by [`ChatStream::session_id`].
    pub async fn chat(
        &self,
        session_id: Option<Uuid>,
        provider_backend_id: ProviderBackendId,
        prompt: String,
    ) -> Result<Arc<ChatStream>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        let stream = on_runtime(async move {
            let chat = inner.chat(session_id, provider_backend_id, prompt).await;
            ChatStream {
                session_id: chat.session_id,
                events: EventStream::pump(chat.stream),
            }
        })
        .await?;
        Ok(Arc::new(stream))
    }

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.available_sessions().await }).await??)
    }

    /// Ids of stored sessions whose user prompts or assistant messages
    /// contain `needle`.
    pub async fn search_sessions(&self, needle: String) -> Result<Vec<Uuid>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.search_sessions(needle).await }).await??)
    }

    pub async fn restore_session(&self, session_id: Uuid) -> Result<Arc<EventStream>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        let stream = on_runtime(async move {
            inner
                .restore_session(session_id)
                .await
                .map(EventStream::pump)
        })
        .await??;
        Ok(Arc::new(stream))
    }

    pub async fn remove_session(&self, session_id: Uuid) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.remove_session(session_id).await }).await??)
    }

    pub async fn cancel_session(&self, session_id: Uuid) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.cancel_session(session_id).await }).await??)
    }

    pub async fn decide_toolcall_permissions(
        &self,
        user_decision: UserDecision,
    ) -> Result<PermissionState, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.decide_toolcall_permissions(user_decision).await })
                .await??,
        )
    }

    pub async fn get_permissions(&self) -> Result<Vec<Permission>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.get_permissions().await }).await??)
    }

    pub async fn delete_permission(&self, prefix: String) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.delete_permission(&prefix).await }).await??)
    }

    pub async fn init_connection(
        &self,
        provider_backend_id: ProviderBackendId,
    ) -> Result<ConnectionPayload, PalomaError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.init_connection(provider_backend_id).await })
            .await??
            .try_into()
    }

    pub async fn finalize_connection(
        &self,
        provider_auth_method: ProviderAuthMethod,
        provider_backend_id: ProviderBackendId,
        payload: String,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move {
            inner
                .finalize_connection(provider_auth_method, provider_backend_id, payload)
                .await
        })
        .await??)
    }

    pub async fn cancel_connection(
        &self,
        provider_backend_id: ProviderBackendId,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.cancel_connection(provider_backend_id).await }).await??)
    }

    pub async fn disconnect_connector(
        &self,
        provider_backend_id: ProviderBackendId,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.disconnect_connector(provider_backend_id).await })
                .await??,
        )
    }

    pub async fn set_model_preference(
        &self,
        provider_backend_id: ProviderBackendId,
        model: String,
        effort: String,
        set_default: bool,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move {
            inner
                .set_model_preference(provider_backend_id, &model, &effort, set_default)
                .await
        })
        .await??)
    }

    pub async fn prefer_model(&self) -> Result<Option<ProviderBackendId>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.prefer_model().await }).await??)
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.available_connectors().await })
                .await??
                .into_iter()
                .map(Connector::from)
                .collect(),
        )
    }

    pub async fn connectors_health_level(&self) -> Result<HealthLevel, PalomaError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.connectors_health_level().await }).await
    }

    pub async fn plugins_health_level(&self) -> Result<HealthLevel, PalomaError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.plugins_health_level().await }).await
    }

    pub async fn list_extension_plugins(&self) -> Result<Vec<ExtensionInfo>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.list_extension_plugins().await })
                .await??
                .into_iter()
                .map(ExtensionInfo::from)
                .collect(),
        )
    }

    pub async fn list_provider_plugins(&self) -> Result<Vec<ProviderInfo>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.list_provider_plugins().await }).await??)
    }

    pub async fn add_extension_plugin(&self, config: Plugin) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.add_extension_plugin(config).await }).await??)
    }

    pub async fn add_provider_plugin(&self, config: Plugin) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.add_provider_plugin(config).await }).await??)
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpPluginInfo>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.list_mcps().await })
            .await??
            .into_iter()
            .map(McpPluginInfo::from)
            .collect())
    }

    /// Start connecting an MCP server. Returns an OAuth handle when the server
    /// requires authorization: open its `auth_url`, then pass the handle to
    /// [`Self::finalize_mcp_connection`]. `None` means no OAuth is needed.
    pub async fn init_mcp_connection(
        &self,
        config: Plugin,
    ) -> Result<Option<Arc<McpOauthSession>>, PalomaError> {
        let inner = Arc::clone(&self.inner);
        let state = on_runtime(async move { inner.init_mcp_connection(config).await }).await??;
        Ok(state.map(|state| {
            Arc::new(McpOauthSession {
                auth_url: state.auth_url().to_owned(),
                inner: Mutex::new(Some(state)),
            })
        }))
    }

    /// Persist and connect the server. For OAuth servers this waits for the
    /// browser approval; firing `token` aborts the wait and drops the listener.
    pub async fn finalize_mcp_connection(
        &self,
        config: Plugin,
        session: Option<Arc<McpOauthSession>>,
        token: Arc<CancelToken>,
    ) -> Result<(), PalomaError> {
        let state = session.as_ref().map(|session| session.take()).transpose()?;
        let inner = Arc::clone(&self.inner);
        let task = async move { inner.finalize_mcp_connection(config, state).await };
        Ok(run_cancellable(&token, task).await??)
    }

    pub async fn update_plugin(
        &self,
        plugin_type: PluginType,
        plugin: Plugin,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.update_plugin(plugin_type, plugin).await }).await??)
    }

    pub async fn remove_plugin(
        &self,
        plugin_type: PluginType,
        name: String,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.remove_plugin(plugin_type, &name).await }).await??)
    }

    pub async fn toggle_plugin(&self, name: String, disabled: bool) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.toggle_plugin(&name, disabled).await }).await??)
    }

    pub async fn toggle_capability(
        &self,
        name: String,
        capability: String,
        facet: CapabilityFacet,
        disabled: bool,
    ) -> Result<(), PalomaError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move {
            inner
                .toggle_capability(&name, &capability, facet, disabled)
                .await
        })
        .await??)
    }
}
