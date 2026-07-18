use std::{
    io::Error,
    process::Stdio,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use futures::{
    SinkExt, StreamExt,
    stream::{self, BoxStream},
};
use log::{error, warn};
use scry_provider_protocol::{
    Bytes, Message, PROTOCOL_VERSION,
    transport::{FramedRead, FramedWrite, length_delimited_codec},
    v1::{
        BackendAuth, BackendHealthStatusRequest, BackendInitErrorRequest, CancelChatRequest,
        CancelConnectionRequest, ChatRequest, ConnectionPayload, FinalizeConnectionRequest,
        HandshakeRequest, HandshakeResponse, HealthStatusRequest, InitBackendRequest,
        InitConnectionRequest, InitializeBackendsRequest, ListModelsRequest, Model, ProviderAuth,
        ProviderAuthMethod, ProviderHealthStatus, RemoveBackendRequest, RequestEvent,
        ResponseEvent, chat_response, finalize_connection_request::Input, request_event,
        response_event::Payload,
    },
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
    time,
};

use crate::{
    HealthStatus, Plugin, PluginArgs,
    db::{AuthKind, Storage},
    entity::ProviderBackendId,
};

const PROVIDER_REQUEST_CHANNEL_CAPACITY: usize = 16;
const PROVIDER_CHAT_CHANNEL_CAPACITY: usize = 128;

pub type ChatStream = BoxStream<'static, chat_response::Payload>;

const DEFAULT_UNARY_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CONNECTION_REQUEST_TIMEOUT: Duration = Duration::from_mins(10);
const BACKEND_INIT_TIMEOUT: Duration = Duration::from_secs(30);

enum Pending {
    Unary(oneshot::Sender<Payload>),
    Stream(mpsc::Sender<chat_response::Payload>),
}

pub struct ProviderPlugin {
    next_event_id: AtomicU64,
    health_status: Arc<AtomicU8>,
    error: Arc<OnceLock<String>>,
    pending: Arc<DashMap<u64, Pending>>,
    writer: mpsc::Sender<RequestEvent>,
    child: Mutex<Child>,
}

impl ProviderPlugin {
    pub fn connect(plugin: &Plugin, storage: Storage) -> Result<Arc<Self>> {
        let mut child = execute_plugin(plugin)?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let health_status = Arc::new(AtomicU8::new(HealthStatus::Starting as u8));
        let error: Arc<OnceLock<String>> = Arc::default();

        // request dispatch
        let (writer, mut writer_rx) =
            mpsc::channel::<RequestEvent>(PROVIDER_REQUEST_CHANNEL_CAPACITY);
        let health = Arc::clone(&health_status);
        let write_error = Arc::clone(&error);
        tokio::spawn(async move {
            let mut output = FramedWrite::new(stdin, length_delimited_codec());
            while let Some(request) = writer_rx.recv().await {
                if let Err(e) = output.send(Bytes::from(request.encode_to_vec())).await {
                    // pipe closed: child is gone
                    let _ = write_error.set(format!("plugin stopped accepting requests: {e}"));
                    health.store(HealthStatus::Unhealthy as u8, Ordering::SeqCst);
                    break;
                }
            }
        });

        // handling response
        let pending: Arc<DashMap<u64, Pending>> = Arc::new(DashMap::new());
        let routes = Arc::clone(&pending);
        let health = Arc::clone(&health_status);
        let read_error = Arc::clone(&error);
        let name = plugin.name.clone();
        tokio::spawn(async move {
            let mut input = FramedRead::new(stdout, length_delimited_codec());
            while let Some(Ok(frame)) = input.next().await {
                let response = match ResponseEvent::decode(frame.freeze()) {
                    Ok(response) => response,
                    Err(e) => {
                        error!("undecodable provider plugin frame: {e}");
                        continue;
                    },
                };
                let Some(payload) = response.payload else {
                    error!(
                        "response {} has no payload: indicate bugs or newer protocol version",
                        response.event_id
                    );
                    routes.remove(&response.event_id);
                    continue;
                };
                handle_response(&name, &routes, &storage, response.event_id, payload).await;
            }
            // EOF: child died.
            let _ = read_error.set("plugin process exited".to_string());
            health.store(HealthStatus::Unhealthy as u8, Ordering::SeqCst);
            routes.clear();
        });

        Ok(Arc::new(Self {
            next_event_id: AtomicU64::default(),
            health_status,
            error,
            pending,
            writer,
            child: Mutex::new(child),
        }))
    }

    pub fn health(&self) -> HealthStatus {
        HealthStatus::from_u8(self.health_status.load(Ordering::Relaxed))
    }

    pub fn plugin_error(&self) -> Option<String> {
        self.error.get().cloned()
    }

    // explicitly shutdown function
    // use on remove_provider such that any arc reference call will also be killed
    pub fn shutdown(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.start_kill();
        }
    }

    async fn request(
        &self,
        backend_id: Option<String>,
        payload: request_event::Payload,
    ) -> Result<Payload> {
        self.request_with_timeout(backend_id, payload, DEFAULT_UNARY_REQUEST_TIMEOUT)
            .await
    }

    async fn request_with_timeout(
        &self,
        backend_id: Option<String>,
        payload: request_event::Payload,
        timeout: Duration,
    ) -> Result<Payload> {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(event_id, Pending::Unary(tx));

        // Defensive guard on child process killed
        if self.health_status.load(Ordering::SeqCst) == HealthStatus::Unhealthy as u8 {
            self.pending.remove(&event_id);
            return Err(ProviderConnectionError::Disconnected);
        }

        let request = RequestEvent {
            event_id,
            backend_id,
            payload: Some(payload),
        };
        if self.writer.send(request).await.is_err() {
            self.pending.remove(&event_id);
            return Err(ProviderConnectionError::Disconnected);
        }

        let Ok(reply) = time::timeout(timeout, rx).await else {
            self.pending.remove(&event_id);
            return Err(ProviderConnectionError::Timeout(timeout));
        };

        match reply.map_err(|_| ProviderConnectionError::Disconnected)? {
            Payload::ProviderError(e) => Err(ProviderConnectionError::Provider(e.error)),
            payload => Ok(payload),
        }
    }
}

/// initialization
impl ProviderPlugin {
    pub async fn handshake(&self) -> Result<HandshakeResponse> {
        match self
            .request(
                None,
                request_event::Payload::HandshakeRequest(HandshakeRequest {}),
            )
            .await?
        {
            Payload::HandshakeResponse(handshake) => {
                if handshake.version != PROTOCOL_VERSION {
                    return Err(ProviderConnectionError::ProtocolVersion {
                        expected: PROTOCOL_VERSION,
                        actual: handshake.version,
                    });
                }
                self.health_status
                    .store(HealthStatus::Running as u8, Ordering::SeqCst);
                Ok(handshake)
            },
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn init_backends(&self, auths: Vec<BackendAuth>) -> Result<()> {
        match self
            .request_with_timeout(
                None,
                request_event::Payload::InitializeBackendsRequest(InitializeBackendsRequest {
                    auths,
                }),
                BACKEND_INIT_TIMEOUT,
            )
            .await?
        {
            Payload::InitializeBackendsResponse(_) => Ok(()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn init_backend(&self, backend_id: String, auth: BackendAuth) -> Result<()> {
        match self
            .request_with_timeout(
                Some(backend_id),
                request_event::Payload::InitBackendRequest(InitBackendRequest { auth: Some(auth) }),
                BACKEND_INIT_TIMEOUT,
            )
            .await?
        {
            Payload::InitBackendResponse(_) => Ok(()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn remove_backend(&self, backend_id: String) -> Result<()> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::RemoveBackendRequest(RemoveBackendRequest {}),
            )
            .await?
        {
            Payload::RemoveBackendResponse(_) => Ok(()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }
}

/// Connection
impl ProviderPlugin {
    pub async fn init_connection(&self, backend_id: String) -> Result<ConnectionPayload> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::InitConnectionRequest(InitConnectionRequest {}),
            )
            .await?
        {
            Payload::InitConnectionResponse(response) => response
                .connection
                .ok_or(ProviderConnectionError::UnexpectedResponse),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn finalize_connection(
        &self,
        auth_kind: ProviderAuthMethod,
        backend_id: String,
        payload: String,
    ) -> Result<ProviderAuth> {
        let request_payload = match auth_kind {
            ProviderAuthMethod::ApiKey => Input::ApiKey(payload),
            ProviderAuthMethod::DeviceCode => Input::TransactionPayload(payload),
            ProviderAuthMethod::BrowserOauth => Input::AuthorizationResponse(payload),
            ProviderAuthMethod::Unknown => {
                return Err(ProviderConnectionError::Provider(
                    "Unexpected plugin auth type.".into(),
                ));
            },
        };
        match self
            .request_with_timeout(
                Some(backend_id),
                request_event::Payload::FinalizeConnectionRequest(FinalizeConnectionRequest {
                    input: Some(request_payload),
                }),
                DEFAULT_CONNECTION_REQUEST_TIMEOUT,
            )
            .await?
        {
            Payload::FinalizeConnectionResponse(response) => response
                .auth
                .ok_or(ProviderConnectionError::UnexpectedResponse),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn cancel_connection(&self, backend_id: String) -> Result<()> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::CancelConnectionRequest(CancelConnectionRequest {}),
            )
            .await?
        {
            Payload::CancelConnectionResponse(_) => Ok(()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }
}

/// runtime
impl ProviderPlugin {
    pub async fn list_models(&self, backend_id: String) -> Result<Vec<Model>> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::ListModelsRequest(ListModelsRequest {}),
            )
            .await?
        {
            Payload::ListModelsResponse(response) => Ok(response.models),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn health_status(&self, backend_id: String) -> Result<HealthStatus> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::HealthStatusRequest(HealthStatusRequest {}),
            )
            .await?
        {
            Payload::HealthStatusResponse(response) => Ok(response.health_status().into()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    /// Health of every initialized backend in this plugin
    pub async fn backend_health_status(&self) -> Result<Vec<HealthStatus>> {
        match self
            .request(
                None,
                request_event::Payload::BackendHealthStatusRequest(BackendHealthStatusRequest {}),
            )
            .await?
        {
            Payload::BackendHealthStatusResponse(response) => Ok(response
                .status
                .into_iter()
                .map(|status| {
                    ProviderHealthStatus::try_from(status)
                        .unwrap_or(ProviderHealthStatus::Unhealthy)
                        .into()
                })
                .collect()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn error(&self, backend_id: String) -> Result<Option<String>> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::BackendInitErrorRequest(BackendInitErrorRequest {}),
            )
            .await?
        {
            Payload::BackendInitErrorResponse(response) => Ok(response.error),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }

    pub async fn chat(&self, backend_id: String, request: ChatRequest) -> Result<ChatStream> {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(PROVIDER_CHAT_CHANNEL_CAPACITY);
        self.pending.insert(event_id, Pending::Stream(tx));

        // Defensive guard on child process killed
        if self.health_status.load(Ordering::SeqCst) == HealthStatus::Unhealthy as u8 {
            self.pending.remove(&event_id);
            return Err(ProviderConnectionError::Disconnected);
        }

        let request = RequestEvent {
            event_id,
            backend_id: Some(backend_id),
            payload: Some(request_event::Payload::ChatRequest(request)),
        };
        if self.writer.send(request).await.is_err() {
            self.pending.remove(&event_id);
            return Err(ProviderConnectionError::Disconnected);
        }

        Ok(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed())
    }

    pub async fn cancel_chat(&self, backend_id: String, session_id: String) -> Result<()> {
        match self
            .request(
                Some(backend_id),
                request_event::Payload::CancelChatRequest(CancelChatRequest { session_id }),
            )
            .await?
        {
            Payload::CancelChatResponse(_) => Ok(()),
            _ => Err(ProviderConnectionError::UnexpectedResponse),
        }
    }
}

async fn handle_response(
    name: &str,
    routes: &DashMap<u64, Pending>,
    storage: &Storage,
    event_id: u64,
    payload: Payload,
) {
    match payload {
        payload @ (Payload::HandshakeResponse(_)
        | Payload::InitializeBackendsResponse(_)
        | Payload::InitBackendResponse(_)
        | Payload::RemoveBackendResponse(_)
        | Payload::InitConnectionResponse(_)
        | Payload::FinalizeConnectionResponse(_)
        | Payload::CancelConnectionResponse(_)
        | Payload::ListModelsResponse(_)
        | Payload::HealthStatusResponse(_)
        | Payload::BackendInitErrorResponse(_)
        | Payload::CancelChatResponse(_)
        | Payload::BackendHealthStatusResponse(_)) => resolve_unary(routes, event_id, payload),
        Payload::ProviderError(e) => match routes.remove(&event_id) {
            Some((_, Pending::Unary(tx))) => {
                let _ = tx.send(Payload::ProviderError(e));
            },
            Some((_, Pending::Stream(tx))) => {
                // defensive guard in case plugin return a provider error for streaming event
                let _ = tx.send(chat_response::Payload::Error(e.error)).await;
            },
            None => warn!("ProviderError response has no pending request. {}", e.error),
        },
        Payload::ChatResponse(chat) => {
            let Some(event) = chat.payload else {
                warn!("chat response {event_id} has no payload");
                return;
            };
            let finished = matches!(
                event,
                chat_response::Payload::Done(_) | chat_response::Payload::Error(_)
            );
            let sender = routes.get(&event_id).and_then(|entry| match entry.value() {
                Pending::Stream(tx) => Some(tx.clone()),
                Pending::Unary(_) => None,
            });
            match sender {
                Some(tx) => {
                    let _ = tx.send(event).await;
                },
                None => warn!("chat response {event_id} has no pending stream"),
            }
            if finished {
                routes.remove(&event_id);
            }
        },
        Payload::AuthUpdateRequest(update) => {
            let id = ProviderBackendId {
                provider_id: name.to_string(),
                backend_id: update.backend_id,
            };
            if let Err(e) = storage
                .update_backend(&id, &AuthKind::Oauth, &update.refresh_token)
                .await
            {
                error!("failed to persist credential for {}: {e}", id.backend_id);
            }
        },
    }
}

fn resolve_unary(routes: &DashMap<u64, Pending>, event_id: u64, payload: Payload) {
    if let Some((_, Pending::Unary(tx))) = routes.remove(&event_id) {
        let _ = tx.send(payload);
    } else {
        warn!("{} response has no pending unary request", payload.kind());
    }
}

fn execute_plugin(plugin: &Plugin) -> Result<Child> {
    let PluginArgs::Local { command, args } = plugin.args.clone() else {
        return Err(ProviderConnectionError::Provider(format!(
            "provider plugin {} must be a local command",
            plugin.name
        )));
    };

    let mut child = Command::new(command)
        .args(args)
        .envs(&plugin.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    if let Some(stderr) = child.stderr.take() {
        let name = plugin.name.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                warn!("provider plugin [{name}] stderr: {line}");
            }
        });
    }

    Ok(child)
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderConnectionError {
    #[error(transparent)]
    Io(#[from] Error),

    #[error("provider plugin exited or closed its transport")]
    Disconnected,

    #[error("provider plugin did not respond within {0:?}")]
    Timeout(Duration),

    #[error("provider plugin speaks protocol version {actual}, host expects {expected}")]
    ProtocolVersion { expected: u64, actual: u64 },

    #[error("unexpected response payload")]
    UnexpectedResponse,

    #[error("provider error: {0}")]
    Provider(String),
}

type Result<T> = std::result::Result<T, ProviderConnectionError>;
