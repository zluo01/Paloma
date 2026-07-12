//! UniFFI bindings for `scry-core`.
//!
//! The core spawns Tokio tasks and drives sockets internally, while UniFFI
//! polls exported futures on foreign (Swift) threads that have no ambient
//! runtime. Every core call is therefore spawned onto the FFI-owned [`RUNTIME`]
//! and only the join handle is awaited on the foreign side; streams are pumped
//! into channels from inside the runtime for the same reason.

// The swift bridge only exists for the macos frontend; elsewhere the crate
// (and its whole dependency tree, moved to a target section) compiles empty.
#![cfg(target_os = "macos")]

mod error;
mod types;

use std::{
    future::Future,
    path::PathBuf,
    pin::pin,
    sync::{Arc, LazyLock, Mutex},
};

use futures::{Stream, StreamExt};
use scry_core::AppContext;
use tokio::{
    runtime::Runtime,
    sync::{Mutex as AsyncMutex, mpsc},
    task::AbortHandle,
};
use uuid::Uuid;

pub use crate::{error::ScryError, types::*};

uniffi::setup_scaffolding!("scry");

/// Route `log` records to ~/Library/Logs/Scry/scry-YYYY-MM-DD.log
/// (per-day files, appended) and to stderr for Xcode/terminal runs. Call
/// once at app startup, before [`ScryApp::new`]; extra calls are no-ops.
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

/// The frontend's sink into the same log file; Swift has no portable
/// path into `log`.
#[uniffi::export]
pub fn log_error(target: String, message: String) {
    log::error!(target: target.as_str(), "{message}");
}

fn log_file(log_path: String) -> Option<std::fs::File> {
    let dir = PathBuf::from(log_path).join("Scry");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("scry-{}.log", chrono::Local::now().format("%Y-%m-%d"));
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
const EVENT_BUFFER: usize = scry_core::RENDER_CHANNEL_CAPACITY;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("scry-core")
        .build()
        .expect("failed to start the scry tokio runtime")
});

/// Run `task` to completion on [`RUNTIME`]; the returned future is safe to
/// poll from any thread.
async fn on_runtime<T, F>(task: F) -> Result<T, ScryError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    RUNTIME
        .spawn(task)
        .await
        .map_err(|e| ScryError::new(e.to_string()))
}

/// Like [`on_runtime`], but parks the task's abort handle in `slot` so a
/// concurrent cancel can abort it; an abort surfaces as a "cancelled"
/// failure.
///
/// TODO: drop this hand-rolled abort plumbing once uniffi's Swift bindings
/// support cancelling async futures, so dropping the future cancels the
/// task instead (tracked in mozilla/uniffi-rs#2771; the docs currently
/// recommend exactly this kind of side-channel cancel).
async fn run_abortable<T, F>(slot: &Mutex<Option<AbortHandle>>, task: F) -> Result<T, ScryError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let handle = RUNTIME.spawn(task);
    *slot.lock().expect("abort slot lock poisoned") = Some(handle.abort_handle());
    let outcome = handle.await;
    slot.lock().expect("abort slot lock poisoned").take();
    outcome.map_err(|join_error| {
        ScryError::new(if join_error.is_cancelled() {
            "cancelled".to_owned()
        } else {
            join_error.to_string()
        })
    })
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
    fn pump(stream: impl Stream<Item = scry_core::RenderEvent> + Send + 'static) -> Self {
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
/// once; [`Self::cancel`] aborts a finalize already in flight.
#[derive(uniffi::Object)]
pub struct McpOauthSession {
    inner: Mutex<Option<scry_core::OAuthCallbackState>>,
    auth_url: String,
    abort: Mutex<Option<AbortHandle>>,
}

#[uniffi::export]
impl McpOauthSession {
    /// URL the user must open in a browser to authorize the connection.
    pub fn auth_url(&self) -> String {
        self.auth_url.clone()
    }

    /// Abort the finalize running with this session: the callback listener
    /// drops, so a late browser approval can no longer add the server. No-op
    /// before finalize starts or after it settles.
    pub fn cancel(&self) {
        if let Some(abort) = self.abort_slot().take() {
            abort.abort();
        }
    }
}

impl McpOauthSession {
    fn take(&self) -> Result<scry_core::OAuthCallbackState, ScryError> {
        self.inner
            .lock()
            .expect("MCP OAuth session lock poisoned")
            .take()
            .ok_or_else(|| ScryError::new("MCP OAuth session was already consumed"))
    }

    fn abort_slot(&self) -> std::sync::MutexGuard<'_, Option<AbortHandle>> {
        self.abort.lock().expect("MCP OAuth abort lock poisoned")
    }
}

/// FFI entry point wrapping [`scry_core::AppContext`].
#[derive(uniffi::Object)]
pub struct ScryApp {
    inner: Arc<AppContext>,
    /// Abort handle for the in-flight connect init/finalize; the flow is
    /// sequential and the UI allows one attempt at a time, so a single
    /// slot suffices (mirroring [`McpOauthSession`]).
    connect_abort: Mutex<Option<AbortHandle>>,
}

impl ScryApp {
    /// [`run_abortable`] on the connect slot, so [`Self::cancel_connection`]
    /// can abort whichever connect phase is in flight.
    async fn on_runtime_cancellable<T, F>(&self, task: F) -> Result<T, ScryError>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        run_abortable(&self.connect_abort, task).await
    }

    fn connect_abort_slot(&self) -> std::sync::MutexGuard<'_, Option<AbortHandle>> {
        self.connect_abort
            .lock()
            .expect("connect abort lock poisoned")
    }
}

#[uniffi::export]
impl ScryApp {
    /// Build the whole core: storage, providers, and the plugin/session/turn
    /// event loops, all living on the FFI-owned runtime.
    #[uniffi::constructor]
    pub async fn new(app_data_path: String) -> Result<Arc<Self>, ScryError> {
        let inner = on_runtime(AppContext::build(PathBuf::from(app_data_path))).await??;
        Ok(Arc::new(Self {
            inner,
            connect_abort: Mutex::new(None),
        }))
    }

    /// Run a launcher search; results arrive on the returned stream.
    pub async fn query(&self, input: String) -> Result<Arc<EventStream>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let stream = on_runtime(async move { EventStream::pump(inner.query(&input)) }).await?;
        Ok(Arc::new(stream))
    }

    /// Run an action off the caller's thread: handlers may spawn processes
    /// or touch the pasteboard, which shouldn't stall the UI thread.
    pub async fn run_query_action(
        &self,
        id: String,
        action: Action,
    ) -> Result<Option<ActionOutcome>, ScryError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.run_query_action(&id, action) }).await
    }

    /// Start (or continue) a chat turn. `session_id` of `None` opens a new
    /// session; the created id is reported by [`ChatStream::session_id`].
    pub async fn chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<Arc<ChatStream>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let stream = on_runtime(async move {
            let chat = inner.chat(session_id, provider_id, prompt).await;
            ChatStream {
                session_id: chat.session_id,
                events: EventStream::pump(chat.stream),
            }
        })
        .await?;
        Ok(Arc::new(stream))
    }

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.available_sessions().await }).await??)
    }

    /// Ids of stored sessions whose user prompts or assistant messages
    /// contain `needle`.
    pub async fn search_sessions(&self, needle: String) -> Result<Vec<Uuid>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.search_sessions(needle).await }).await??)
    }

    pub async fn restore_session(&self, session_id: Uuid) -> Result<Arc<EventStream>, ScryError> {
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

    pub async fn remove_session(&self, session_id: Uuid) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.remove_session(session_id).await }).await??)
    }

    pub async fn cancel_session(&self, session_id: Uuid) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.cancel_session(session_id).await }).await??)
    }

    pub async fn decide_toolcall_permissions(
        &self,
        user_decision: UserDecision,
    ) -> Result<PermissionState, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.decide_toolcall_permissions(user_decision).await })
                .await??,
        )
    }

    pub async fn get_permissions(&self) -> Result<Vec<Permission>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.get_permissions().await }).await??)
    }

    pub async fn delete_permission(&self, prefix: String) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.delete_permission(&prefix).await }).await??)
    }

    /// Start connecting a provider; [`Self::cancel_connection`] aborts the
    /// in-flight call.
    pub async fn init_connection(&self, provider_id: ProviderId) -> Result<Connection, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(self
            .on_runtime_cancellable(async move { inner.init_connection(provider_id).await })
            .await??
            .into())
    }

    /// Complete the connection; for device-code providers this polls until
    /// the user approves in the browser. [`Self::cancel_connection`] aborts
    /// the wait.
    pub async fn finalize_connection(
        &self,
        provider_id: ProviderId,
        payload: Connection,
    ) -> Result<(), ScryError> {
        let payload: scry_core::Connection = payload.try_into()?;
        let inner = Arc::clone(&self.inner);
        Ok(self
            .on_runtime_cancellable(
                async move { inner.finalize_connection(provider_id, payload).await },
            )
            .await??)
    }

    /// Abort the in-flight connect init or finalize, if any: the awaiting
    /// call returns a "cancelled" failure. No-op when nothing is running.
    pub fn cancel_connection(&self) {
        if let Some(abort) = self.connect_abort_slot().take() {
            abort.abort();
        }
    }

    pub async fn disconnect_connector(&self, provider_id: ProviderId) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.disconnect_connector(provider_id).await }).await??)
    }

    pub async fn set_model_preference(
        &self,
        provider_id: ProviderId,
        model: String,
        effort: String,
    ) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move {
            inner
                .set_model_preference(provider_id, &model, &effort)
                .await
        })
        .await??)
    }

    pub async fn prefer_model(&self) -> Result<Option<ProviderId>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.prefer_model().await }).await??)
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.available_connectors().await }).await??)
    }

    pub async fn connectors_health_level(&self) -> Result<HealthLevel, ScryError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.connectors_health_level().await }).await
    }

    pub async fn plugins_health_level(&self) -> Result<HealthLevel, ScryError> {
        let inner = Arc::clone(&self.inner);
        on_runtime(async move { inner.plugins_health_level().await }).await
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpServer>, ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.list_mcps().await }).await??)
    }

    /// Start connecting an MCP server. Returns an OAuth handle when the server
    /// requires authorization: open its `auth_url`, then pass the handle to
    /// [`Self::finalize_mcp_connection`]. `None` means no OAuth is needed.
    pub async fn init_mcp_connection(
        &self,
        config: Plugin,
    ) -> Result<Option<Arc<McpOauthSession>>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let state = on_runtime(async move { inner.init_mcp_connection(config).await }).await??;
        Ok(state.map(|state| {
            Arc::new(McpOauthSession {
                auth_url: state.auth_url().to_owned(),
                inner: Mutex::new(Some(state)),
                abort: Mutex::new(None),
            })
        }))
    }

    /// Persist and connect the server. For OAuth servers this waits for the
    /// browser approval; [`McpOauthSession::cancel`] aborts the wait.
    pub async fn finalize_mcp_connection(
        &self,
        config: Plugin,
        session: Option<Arc<McpOauthSession>>,
    ) -> Result<(), ScryError> {
        let state = session.as_ref().map(|session| session.take()).transpose()?;
        let inner = Arc::clone(&self.inner);
        let task = async move { inner.finalize_mcp_connection(config, state).await };
        match &session {
            Some(session) => Ok(run_abortable(&session.abort, task).await??),
            None => Ok(on_runtime(task).await??),
        }
    }

    pub async fn update_plugin(
        &self,
        plugin_type: PluginType,
        plugin: Plugin,
    ) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.update_plugin(plugin_type, plugin).await }).await??)
    }

    pub async fn remove_plugin(
        &self,
        plugin_type: PluginType,
        name: String,
    ) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.remove_plugin(plugin_type, &name).await }).await??)
    }

    pub async fn toggle_plugin(&self, name: String, disabled: bool) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.toggle_plugin(&name, disabled).await }).await??)
    }
}
