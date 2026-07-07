//! UniFFI bindings for `scry-core`.
//!
//! The core spawns Tokio tasks and drives sockets internally, while UniFFI
//! polls exported futures on foreign (Swift) threads that have no ambient
//! runtime. Every core call is therefore spawned onto the FFI-owned [`RUNTIME`]
//! and only the join handle is awaited on the foreign side; streams are pumped
//! into channels from inside the runtime for the same reason.

mod error;
mod types;

use std::{
    future::Future,
    pin::pin,
    sync::{Arc, LazyLock, Mutex},
};

use futures::{Stream, StreamExt};
use scry_core::AppContext;
use tokio::{
    runtime::Runtime,
    sync::{Mutex as AsyncMutex, mpsc},
};
use uuid::Uuid;

pub use crate::{error::ScryError, types::*};

uniffi::setup_scaffolding!("scry");

/// Matches core's `RENDER_CHANNEL_CAPACITY` so the pump adds no extra slack.
const EVENT_BUFFER: usize = 32;

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
    RUNTIME.spawn(task).await.map_err(|e| ScryError::Runtime {
        message: e.to_string(),
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
/// once; dropping it aborts the flow.
#[derive(uniffi::Object)]
pub struct McpOauthSession {
    inner: Mutex<Option<scry_core::OAuthCallbackState>>,
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
    fn take(&self) -> Result<scry_core::OAuthCallbackState, ScryError> {
        self.inner
            .lock()
            .expect("MCP OAuth session lock poisoned")
            .take()
            .ok_or_else(|| ScryError::InvalidArgument {
                message: "MCP OAuth session was already consumed".to_owned(),
            })
    }
}

/// FFI entry point wrapping [`scry_core::AppContext`].
#[derive(uniffi::Object)]
pub struct ScryApp {
    inner: Arc<AppContext>,
}

#[uniffi::export]
impl ScryApp {
    /// Build the whole core: storage, providers, and the plugin/session/turn
    /// event loops, all living on the FFI-owned runtime.
    #[uniffi::constructor]
    pub async fn new() -> Result<Arc<Self>, ScryError> {
        let inner = on_runtime(AppContext::build()).await??;
        Ok(Arc::new(Self { inner }))
    }

    /// Run a launcher search; results arrive on the returned stream.
    pub async fn query(&self, input: String) -> Result<Arc<EventStream>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let stream = on_runtime(async move { EventStream::pump(inner.query(&input)) }).await?;
        Ok(Arc::new(stream))
    }

    pub fn run_query_action(&self, id: String, action: Action) -> Option<ActionOutcome> {
        self.inner
            .run_query_action(&id, action.into())
            .map(Into::into)
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
            let chat = inner.chat(session_id, provider_id.into(), prompt).await;
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
        let sessions = on_runtime(async move { inner.available_sessions().await }).await??;
        Ok(sessions.into_iter().map(Into::into).collect())
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
        let state = on_runtime(async move {
            inner
                .decide_toolcall_permissions(user_decision.into())
                .await
        })
        .await??;
        Ok(state.into())
    }

    pub async fn get_permissions(&self) -> Result<Vec<Permission>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let permissions = on_runtime(async move { inner.get_permissions().await }).await??;
        Ok(permissions.into_iter().map(Into::into).collect())
    }

    pub async fn delete_permission(&self, prefix: String) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.delete_permission(&prefix).await }).await??)
    }

    pub async fn init_connection(&self, provider_id: ProviderId) -> Result<Connection, ScryError> {
        let inner = Arc::clone(&self.inner);
        let connection =
            on_runtime(async move { inner.init_connection(provider_id.into()).await }).await??;
        Ok(connection.into())
    }

    pub async fn finalize_connection(
        &self,
        provider_id: ProviderId,
        payload: Connection,
    ) -> Result<(), ScryError> {
        let payload: scry_core::Connection = payload.try_into()?;
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.finalize_connection(provider_id.into(), payload).await })
                .await??,
        )
    }

    pub async fn disconnect_connector(&self, provider_id: ProviderId) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.disconnect_connector(provider_id.into()).await })
                .await??,
        )
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
                .set_model_preference(provider_id.into(), &model, &effort)
                .await
        })
        .await??)
    }

    pub async fn prefer_model(&self) -> Result<Option<ProviderId>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let provider = on_runtime(async move { inner.prefer_model().await }).await??;
        Ok(provider.map(Into::into))
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let connectors = on_runtime(async move { inner.available_connectors().await }).await??;
        Ok(connectors.into_iter().map(Into::into).collect())
    }

    pub async fn connectors_health_level(&self) -> Result<HealthLevel, ScryError> {
        let inner = Arc::clone(&self.inner);
        let level = on_runtime(async move { inner.connectors_health_level().await }).await?;
        Ok(level.into())
    }

    pub async fn plugins_health_level(&self) -> Result<HealthLevel, ScryError> {
        let inner = Arc::clone(&self.inner);
        let level = on_runtime(async move { inner.plugins_health_level().await }).await?;
        Ok(level.into())
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpServer>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let servers = on_runtime(async move { inner.list_mcps().await }).await??;
        Ok(servers.into_iter().map(Into::into).collect())
    }

    /// Start connecting an MCP server. Returns an OAuth handle when the server
    /// requires authorization: open its `auth_url`, then pass the handle to
    /// [`Self::finalize_mcp_connection`]. `None` means no OAuth is needed.
    pub async fn init_mcp_connection(
        &self,
        config: Plugin,
    ) -> Result<Option<Arc<McpOauthSession>>, ScryError> {
        let inner = Arc::clone(&self.inner);
        let state =
            on_runtime(async move { inner.init_mcp_connection(config.into()).await }).await??;
        Ok(state.map(|state| {
            Arc::new(McpOauthSession {
                auth_url: state.auth_url().to_owned(),
                inner: Mutex::new(Some(state)),
            })
        }))
    }

    pub async fn finalize_mcp_connection(
        &self,
        config: Plugin,
        session: Option<Arc<McpOauthSession>>,
    ) -> Result<(), ScryError> {
        let state = session.map(|session| session.take()).transpose()?;
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.finalize_mcp_connection(config.into(), state).await })
                .await??,
        )
    }

    pub async fn update_plugin(
        &self,
        plugin_type: PluginType,
        plugin: Plugin,
    ) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.update_plugin(plugin_type.into(), plugin.into()).await })
                .await??,
        )
    }

    pub async fn remove_plugin(
        &self,
        plugin_type: PluginType,
        name: String,
    ) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(
            on_runtime(async move { inner.remove_plugin(plugin_type.into(), &name).await })
                .await??,
        )
    }

    pub async fn toggle_plugin(&self, name: String, disabled: bool) -> Result<(), ScryError> {
        let inner = Arc::clone(&self.inner);
        Ok(on_runtime(async move { inner.toggle_plugin(&name, disabled).await }).await??)
    }
}
