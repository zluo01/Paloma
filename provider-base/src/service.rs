use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use scry_provider_protocol::{
    Bytes, Message, PROTOCOL_VERSION, v1 as proto,
    v1::{
        ProviderAuth, RequestEvent, ResponseEvent, chat_response,
        finalize_connection_request::Input, request_event::Payload, response_event,
    },
};
use scry_utils::transport::{FramedRead, FramedWrite, VarintDelimitedCodec};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    Dispatcher, ProviderAuthenticator, ProviderCache, ProviderClient, ProviderError,
    ProviderService, Result,
};

const PROVIDER_INNER_CHANNEL_CAPACITY: usize = 128;

#[async_trait::async_trait]
pub trait ProviderRuntime: Send + Sync + 'static {
    fn provider_id(&self) -> &str;

    fn description(&self) -> &str;

    fn backends(&self) -> Vec<proto::Backend>;

    fn connector(&self, backend_id: &str) -> Option<&dyn ProviderAuthenticator>;

    async fn build_runtime(
        &self,
        backend_id: &str,
        auth: &ProviderAuth,
        request: &reqwest::Client,
        cache: &Arc<ProviderCache>,
        dispatcher: &Dispatcher,
    ) -> Result<Arc<dyn ProviderClient>>;
}

type Runtimes = Arc<RwLock<HashMap<String, Arc<dyn ProviderClient>>>>;

/// The [`ProviderService`] shared by every provider plugin: routes protocol
/// requests to a [`ProviderRuntime`]'s connectors and runtimes and owns the
/// per-backend connection and per-session chat task bookkeeping.
pub struct ProviderRuntimeService<F> {
    group: Arc<F>,
    request: reqwest::Client,
    cache: Arc<ProviderCache>,
    runtimes: Runtimes,
    cancellation: Arc<DashMap<String, CancellationToken>>,
}

impl<F: ProviderRuntime> ProviderRuntimeService<F> {
    pub fn new(family: F, request: reqwest::Client) -> Self {
        Self {
            group: Arc::new(family),
            request,
            cache: ProviderCache::new(),
            runtimes: Arc::new(RwLock::new(HashMap::new())),
            cancellation: Arc::new(DashMap::new()),
        }
    }

    /// Run the plugin's stdin/stdout protocol loop until the host closes
    /// stdin; should only be called under a tokio runtime.
    pub async fn serve(self) -> Result<()> {
        let mut input = FramedRead::new(tokio::io::stdin(), VarintDelimitedCodec);
        let (tx, mut rx) = mpsc::channel::<ResponseEvent>(PROVIDER_INNER_CHANNEL_CAPACITY);

        // Writer task: sole owner of stdout. Exits once every sender is dropped.
        let writer = tokio::spawn(async move {
            let mut output = FramedWrite::new(tokio::io::stdout(), VarintDelimitedCodec);
            while let Some(response) = rx.recv().await {
                output.send(Bytes::from(response.encode_to_vec())).await?;
            }
            Ok::<_, std::io::Error>(())
        });

        while let Some(frame) = input.next().await {
            let request = RequestEvent::decode(frame?.freeze())?;
            let dispatcher = Dispatcher::new(request.event_id, tx.clone());

            let Some(payload) = request.payload else {
                error!(
                    "request {} has no payload: host bug or newer protocol version",
                    request.event_id
                );
                dispatcher
                    .send(response_event::Payload::ProviderError(
                        proto::ProviderError {
                            error: "unsupported or missing request payload".into(),
                        },
                    ))
                    .await;
                continue;
            };

            self.handle(request.backend_id, payload, dispatcher).await
        }

        // stdin EOF: drop the root sender and the service (runtimes may hold a
        // Dispatcher) so the writer drains in-flight responses and exits.
        drop(tx);
        drop(self);
        writer
            .await
            .map_err(|e| ProviderError::Other(format!("writer task panicked: {e}")))??;
        Ok(())
    }

    fn runtime(&self, backend_id: Option<&str>) -> Result<Arc<dyn ProviderClient>> {
        backend_id
            .and_then(|id| self.runtimes.read().unwrap().get(id).cloned())
            .ok_or_else(|| {
                ProviderError::Other(format!(
                    "backend {} is not initialized",
                    backend_id.unwrap_or("<missing>")
                ))
            })
    }

    /// Bad entries indicate a host bug; skip them with a warning instead of
    /// discarding the healthy backends.
    async fn init_backends(&self, auths: Vec<proto::BackendAuth>, dispatcher: &Dispatcher) {
        for backend_auth in auths {
            let backend_id = backend_auth.backend_id.clone();
            if let Err(e) = self.init_backend(Some(backend_auth), dispatcher).await {
                warn!("fail to initialize backend {backend_id}; skipping. {e}");
            }
        }
    }

    async fn init_backend(
        &self,
        backend_auth: Option<proto::BackendAuth>,
        dispatcher: &Dispatcher,
    ) -> Result<Option<String>> {
        let backend_auth = required(backend_auth, "auth")?;
        let auth = required(backend_auth.auth, "auth")?;
        let runtime = self
            .group
            .build_runtime(
                &backend_auth.backend_id,
                &auth,
                &self.request,
                &self.cache,
                dispatcher,
            )
            .await?;
        let error = runtime.error();
        if self
            .runtimes
            .write()
            .unwrap()
            .insert(backend_auth.backend_id.clone(), runtime)
            .is_some()
        {
            info!(
                "backend {} re-initialized, replacing the existing runtime.",
                backend_auth.backend_id
            );
        }
        Ok(error)
    }

    async fn handle_init_connection(
        &self,
        backend_id: Option<String>,
    ) -> Result<proto::ConnectionPayload> {
        let connector = backend_id
            .as_deref()
            .and_then(|id| self.group.connector(id))
            .ok_or_else(|| {
                ProviderError::Other(format!(
                    "unknown backend: {}",
                    backend_id.as_deref().unwrap_or("<missing>")
                ))
            })?;

        connector.init_connection().await
    }
}

#[async_trait::async_trait]
impl<F: ProviderRuntime> ProviderService for ProviderRuntimeService<F> {
    async fn handle(&self, backend_id: Option<String>, payload: Payload, dispatcher: Dispatcher) {
        match payload {
            Payload::HandshakeRequest(_) => {
                dispatcher
                    .send(response_event::Payload::HandshakeResponse(
                        proto::HandshakeResponse {
                            version: PROTOCOL_VERSION,
                            provider_id: self.group.provider_id().into(),
                            description: self.group.description().into(),
                            backends: self.group.backends(),
                        },
                    ))
                    .await;
            },
            Payload::InitializeBackendsRequest(request) => {
                self.init_backends(request.auths, &dispatcher).await;
                dispatcher
                    .send(response_event::Payload::InitializeBackendsResponse(
                        proto::InitializeBackendsResponse {},
                    ))
                    .await;
            },
            Payload::InitBackendRequest(request) => {
                let response = match self.init_backend(request.auth, &dispatcher).await {
                    Ok(None) => {
                        response_event::Payload::InitBackendResponse(proto::InitBackendResponse {})
                    },
                    Ok(Some(error)) => {
                        response_event::Payload::ProviderError(proto::ProviderError { error })
                    },
                    Err(e) => response_event::Payload::ProviderError(e.into()),
                };
                dispatcher.send(response).await;
            },
            Payload::RemoveBackendRequest(_) => {
                let response = match backend_id.as_deref() {
                    Some(id) => {
                        // unlikely to happen through UI workflow, defensive guard.
                        if let Some((_, token)) = self.cancellation.remove(id) {
                            token.cancel();
                        }
                        if self.runtimes.write().unwrap().remove(id).is_none() {
                            warn!("backend {id} was not connected; nothing to remove.");
                        }
                        response_event::Payload::RemoveBackendResponse(
                            proto::RemoveBackendResponse {},
                        )
                    },
                    None => response_event::Payload::ProviderError(proto::ProviderError {
                        error: "missing required field: backend_id".into(),
                    }),
                };
                dispatcher.send(response).await;
            },
            Payload::InitConnectionRequest(_) => {
                let response = match self.handle_init_connection(backend_id).await {
                    Ok(connection) => response_event::Payload::InitConnectionResponse(
                        proto::InitConnectionResponse {
                            connection: Some(connection),
                        },
                    ),
                    Err(e) => response_event::Payload::ProviderError(e.into()),
                };
                dispatcher.send(response).await;
            },
            Payload::FinalizeConnectionRequest(request) => {
                let family = Arc::clone(&self.group);
                let runtimes = Arc::clone(&self.runtimes);
                let cancellation = Arc::clone(&self.cancellation);
                let key = backend_id.clone().unwrap_or_default();
                let token = CancellationToken::new();
                tokio::spawn({
                    let key = key.clone();
                    let token = token.clone();
                    async move {
                        tokio::select! {
                            _ = token.cancelled() => {
                                 dispatcher
                                    .send(response_event::Payload::ProviderError(
                                        proto::ProviderError { error: "connection cancelled".into() },
                                    ))
                                    .await;
                            },
                            result = finalize_connection(
                                family,
                                runtimes,
                                backend_id,
                                request.input,
                            ) => {
                                let response = match result {
                                    Ok(auth) => {
                                        response_event::Payload::FinalizeConnectionResponse(
                                            proto::FinalizeConnectionResponse { auth: Some(auth) },
                                        )
                                    },
                                    Err(e) => response_event::Payload::ProviderError(e.into()),
                                };
                                dispatcher.send(response).await;
                                cancellation.remove(&key);
                            },
                        }
                    }
                });
                if let Some(previous) = self.cancellation.insert(key.clone(), token) {
                    error!(
                        "backend {key} already had an in-flight connection flow. This indicates a bug. Cancelling the previous task."
                    );
                    previous.cancel();
                }
            },
            Payload::CancelConnectionRequest(_) => {
                if let Some(backend_id) = backend_id.as_deref()
                    && let Some((_, token)) = self.cancellation.remove(backend_id)
                {
                    token.cancel();
                }
                dispatcher
                    .send(response_event::Payload::CancelConnectionResponse(
                        proto::CancelConnectionResponse {},
                    ))
                    .await;
            },
            Payload::ChatRequest(request) => match self.runtime(backend_id.as_deref()) {
                Ok(runtime) => {
                    let session_id = request.session_id.clone();
                    let cancellation = Arc::clone(&self.cancellation);
                    let chat_dispatcher = dispatcher.clone();
                    let token = CancellationToken::new();
                    tokio::spawn({
                        let session_id = session_id.clone();
                        let token = token.clone();
                        async move {
                            tokio::select! {
                                _ = token.cancelled() => {
                                    chat_dispatcher
                                        .send_chat_event(chat_response::Payload::Error(
                                            "chat cancelled".into(),
                                        ))
                                        .await;
                                },
                                result = runtime.chat(request, chat_dispatcher.clone()) => {
                                    if let Err(e) = result {
                                        chat_dispatcher
                                            .send_chat_event(chat_response::Payload::Error(
                                                e.to_string(),
                                            ))
                                            .await;
                                    }
                                    cancellation.remove(&session_id);
                                },
                            }
                        }
                    });
                    if let Some(previous) = self.cancellation.insert(session_id.clone(), token) {
                        error!(
                            "session {session_id} already had an in-flight chat; cancelling the previous task. This indicates a bug."
                        );
                        previous.cancel();
                    }
                },
                Err(e) => {
                    dispatcher
                        .send_chat_event(chat_response::Payload::Error(e.to_string()))
                        .await;
                },
            },
            Payload::ListModelsRequest(_) => match self.runtime(backend_id.as_deref()) {
                Ok(runtime) => {
                    let models = runtime.models().await.unwrap_or_default();
                    dispatcher
                        .send(response_event::Payload::ListModelsResponse(
                            proto::ListModelsResponse { models },
                        ))
                        .await;
                },
                Err(e) => {
                    dispatcher
                        .send(response_event::Payload::ProviderError(e.into()))
                        .await;
                },
            },
            Payload::HealthStatusRequest(_) => match self.runtime(backend_id.as_deref()) {
                Ok(runtime) => {
                    dispatcher
                        .send(response_event::Payload::HealthStatusResponse(
                            proto::HealthStatusResponse {
                                health_status: runtime.health_status() as i32,
                            },
                        ))
                        .await;
                },
                Err(e) => {
                    dispatcher
                        .send(response_event::Payload::ProviderError(e.into()))
                        .await;
                },
            },
            Payload::BackendInitErrorRequest(_) => {
                let error = match self.runtime(backend_id.as_deref()) {
                    Ok(runtime) => runtime.error(),
                    Err(e) => Some(e.to_string()),
                };
                dispatcher
                    .send(response_event::Payload::BackendInitErrorResponse(
                        proto::BackendInitErrorResponse { error },
                    ))
                    .await;
            },
            Payload::CancelChatRequest(request) => {
                match self.cancellation.remove(&request.session_id) {
                    Some((_, token)) => token.cancel(),
                    // do not throw error for cancel chat, nothing to cancel is no-op
                    None => warn!("no in-flight chat for session {}", request.session_id),
                }
                dispatcher
                    .send(response_event::Payload::CancelChatResponse(
                        proto::CancelChatResponse {},
                    ))
                    .await;
            },
            Payload::BackendHealthStatusRequest(_) => {
                let status: Vec<i32> = self
                    .runtimes
                    .read()
                    .unwrap()
                    .values()
                    .map(|runtime| runtime.health_status() as i32)
                    .collect();
                dispatcher
                    .send(response_event::Payload::BackendHealthStatusResponse(
                        proto::BackendHealthStatusResponse { status },
                    ))
                    .await;
            },
        }
    }
}

async fn finalize_connection<F: ProviderRuntime>(
    family: Arc<F>,
    runtimes: Runtimes,
    backend_id: Option<String>,
    connection: Option<Input>,
) -> Result<ProviderAuth> {
    let backend_id = required(backend_id, "backend_id")?;
    let connector = family
        .connector(&backend_id)
        .ok_or_else(|| ProviderError::Other(format!("unknown backend: {backend_id}")))?;

    if runtimes.read().unwrap().contains_key(&backend_id) {
        return Err(ProviderError::Other(format!(
            "backend {backend_id} is already connected"
        )));
    }

    let connection = required(connection, "connection")?;

    connector.finalize_connection(connection).await
}

fn required<T>(field: Option<T>, name: &'static str) -> Result<T> {
    field.ok_or_else(|| ProviderError::Other(format!("missing required field: {name}")))
}
