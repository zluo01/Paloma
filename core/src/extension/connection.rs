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
use futures::{SinkExt, StreamExt};
use log::{error, warn};
use scry_extension_protocol::{
    Bytes, Message, PROTOCOL_VERSION,
    v1::{
        Action, HandshakeRequest, HandshakeResponse, Item, RequestEvent, ResponseEvent,
        RunActionRequest, SearchRequest, request_event, response_event::Payload,
        run_action_response::Behavior,
    },
};
use scry_utils::transport::{FramedRead, FramedWrite, VarintDelimitedCodec};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
    time,
};

use crate::{HealthStatus, Plugin, PluginArgs};

const EXTENSION_REQUEST_CHANNEL_CAPACITY: usize = 16;
const DEFAULT_UNARY_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ExtensionPlugin {
    next_event_id: AtomicU64,
    health_status: Arc<AtomicU8>,
    error: Arc<OnceLock<String>>,
    pending: Arc<DashMap<u64, oneshot::Sender<Payload>>>,
    writer: mpsc::Sender<RequestEvent>,
    child: Mutex<Child>,
}

impl ExtensionPlugin {
    pub fn connect(plugin: &Plugin) -> Result<Arc<Self>> {
        let mut child = execute_plugin(plugin)?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let health_status = Arc::new(AtomicU8::new(HealthStatus::Starting as u8));
        let error: Arc<OnceLock<String>> = Arc::default();

        // request dispatch
        let (writer, mut writer_rx) =
            mpsc::channel::<RequestEvent>(EXTENSION_REQUEST_CHANNEL_CAPACITY);
        let health = Arc::clone(&health_status);
        let write_error = Arc::clone(&error);
        tokio::spawn(async move {
            let mut output = FramedWrite::new(stdin, VarintDelimitedCodec);
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
        let pending: Arc<DashMap<u64, oneshot::Sender<Payload>>> = Arc::new(DashMap::new());
        let routes = Arc::clone(&pending);
        let health = Arc::clone(&health_status);
        let read_error = Arc::clone(&error);
        tokio::spawn(async move {
            let mut input = FramedRead::new(stdout, VarintDelimitedCodec);
            while let Some(Ok(frame)) = input.next().await {
                let response = match ResponseEvent::decode(frame.freeze()) {
                    Ok(response) => response,
                    Err(e) => {
                        error!("undecodable extension plugin frame: {e}");
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
                match routes.remove(&response.event_id) {
                    Some((_, tx)) => {
                        let _ = tx.send(payload);
                    },
                    None => warn!(
                        "response {} has no pending unary request",
                        response.event_id
                    ),
                }
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
    // use on remove such that any arc reference call will also be killed
    pub fn shutdown(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.start_kill();
        }
    }

    async fn request(
        &self,
        capability_id: Option<String>,
        payload: request_event::Payload,
    ) -> Result<Payload> {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(event_id, tx);

        // Defensive guard on child process killed
        if self.health_status.load(Ordering::SeqCst) == HealthStatus::Unhealthy as u8 {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Disconnected);
        }

        let request = RequestEvent {
            event_id,
            capability_id,
            payload: Some(payload),
        };
        if self.writer.send(request).await.is_err() {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Disconnected);
        }

        let Ok(reply) = time::timeout(DEFAULT_UNARY_REQUEST_TIMEOUT, rx).await else {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Timeout(
                DEFAULT_UNARY_REQUEST_TIMEOUT,
            ));
        };

        match reply.map_err(|_| ExtensionConnectionError::Disconnected)? {
            Payload::ExtensionError(e) => Err(ExtensionConnectionError::Extension(e.error)),
            payload => Ok(payload),
        }
    }

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
                    return Err(ExtensionConnectionError::ProtocolVersion {
                        expected: PROTOCOL_VERSION,
                        actual: handshake.version,
                    });
                }
                self.health_status
                    .store(HealthStatus::Running as u8, Ordering::SeqCst);
                Ok(handshake)
            },
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }

    pub async fn search(&self, capability_id: String, input: String) -> Result<Vec<Item>> {
        match self
            .request(
                Some(capability_id),
                request_event::Payload::SearchRequest(SearchRequest { input }),
            )
            .await?
        {
            Payload::SearchResponse(response) => Ok(response.items),
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }

    pub async fn run_search_action(
        &self,
        capability_id: String,
        action: Action,
    ) -> Result<Behavior> {
        match self
            .request(
                Some(capability_id),
                request_event::Payload::RunActionRequest(RunActionRequest {
                    action: Some(action),
                }),
            )
            .await?
        {
            Payload::RunActionResponse(response) => response
                .behavior
                .ok_or(ExtensionConnectionError::UnexpectedResponse),
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }
}

fn execute_plugin(plugin: &Plugin) -> Result<Child> {
    let PluginArgs::Local { command, args } = plugin.args.clone() else {
        return Err(ExtensionConnectionError::Extension(format!(
            "extension plugin {} must be a local command",
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
                warn!("extension plugin [{name}] stderr: {line}");
            }
        });
    }

    Ok(child)
}

#[derive(Debug, thiserror::Error)]
pub enum ExtensionConnectionError {
    #[error(transparent)]
    Io(#[from] Error),

    #[error("extension plugin exited or closed its transport")]
    Disconnected,

    #[error("extension plugin did not respond within {0:?}")]
    Timeout(Duration),

    #[error("extension plugin speaks protocol version {actual}, host expects {expected}")]
    ProtocolVersion { expected: u64, actual: u64 },

    #[error("unexpected response payload")]
    UnexpectedResponse,

    #[error("extension error: {0}")]
    Extension(String),
}

type Result<T> = std::result::Result<T, ExtensionConnectionError>;
