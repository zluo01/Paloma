use std::{collections::HashMap, io, sync::Arc};

use dashmap::DashMap;
use paloma_extension_protocol::{
    PROTOCOL_VERSION,
    v1::{
        CancelToolResponse, Capability as CapabilityMeta, ExtensionError, HandshakeResponse,
        InvokeToolResponse, RequestEvent, ResponseEvent, RunActionResponse, SearchFacet,
        SearchResponse, request_event, response_event,
    },
};
use paloma_utils::transport::serve_plugin;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Capability;

const EXTENSION_INNER_CHANNEL_CAPACITY: usize = 32;

pub struct ExtensionService {
    extension_id: String,
    description: String,
    author: Option<String>,
    homepage: Option<String>,
    handlers: Arc<HashMap<String, Box<dyn Capability>>>,
    cancellation: Arc<DashMap<String, CancellationToken>>,
}

impl ExtensionService {
    pub fn new(
        extension_id: impl Into<String>,
        description: impl Into<String>,
        author: Option<String>,
        homepage: Option<String>,
        capabilities: Vec<Box<dyn Capability>>,
    ) -> Self {
        let mut handlers = HashMap::new();
        for handler in capabilities {
            if let Some(previous) = handlers.insert(handler.id().to_string(), handler) {
                panic!("duplicate capability id: {}", previous.id());
            }
        }
        Self {
            extension_id: extension_id.into(),
            description: description.into(),
            author,
            homepage,
            handlers: Arc::new(handlers),
            cancellation: Arc::new(DashMap::new()),
        }
    }

    /// Run the plugin's stdin/stdout protocol loop until the host closes
    /// stdin; should only be called under a tokio runtime.
    pub async fn serve(self) -> io::Result<()> {
        serve_plugin(
            EXTENSION_INNER_CHANNEL_CAPACITY,
            async move |request: RequestEvent, tx: mpsc::Sender<ResponseEvent>| {
                self.handle(request, tx).await
            },
        )
        .await
    }

    async fn handle(&self, request: RequestEvent, tx: mpsc::Sender<ResponseEvent>) {
        let RequestEvent {
            event_id,
            capability_id,
            payload,
        } = request;

        let Some(payload) = payload else {
            let _ = tx
                .send(response(
                    event_id,
                    error_payload("unsupported or missing request payload"),
                ))
                .await;
            return;
        };

        match payload {
            request_event::Payload::HandshakeRequest(_) => {
                let mut capabilities: Vec<_> = self
                    .handlers
                    .values()
                    .map(|capability| CapabilityMeta {
                        capability_id: capability.id().to_string(),
                        description: capability.description().to_string(),
                        search: capability.search_handler().map(|_| SearchFacet {}),
                        tool: capability.tool_handler().map(|tool| tool.facet()),
                    })
                    .collect();
                capabilities.sort_by(|a, b| a.capability_id.cmp(&b.capability_id));
                let payload = response_event::Payload::HandshakeResponse(HandshakeResponse {
                    version: PROTOCOL_VERSION,
                    extension_id: self.extension_id.clone(),
                    description: self.description.clone(),
                    author: self.author.clone(),
                    homepage: self.homepage.clone(),
                    capabilities,
                });
                let _ = tx.send(response(event_id, payload)).await;
            },
            request_event::Payload::SearchRequest(request) => {
                let handlers = Arc::clone(&self.handlers);
                tokio::spawn(async move {
                    let joined = tokio::task::spawn_blocking(move || {
                        match handler(&handlers, capability_id) {
                            Ok(capability) => match capability.search_handler() {
                                Some(search) => {
                                    response_event::Payload::SearchResponse(SearchResponse {
                                        items: search.search(&request.input),
                                    })
                                },
                                None => error_payload(&format!(
                                    "capability {} does not implement search handler",
                                    capability.id()
                                )),
                            },
                            Err(error) => error_payload(&error),
                        }
                    })
                    .await;

                    let payload = match joined {
                        Ok(payload) => payload,
                        Err(e) => error_payload(&format!("search handler panicked: {e}")),
                    };
                    let _ = tx.send(response(event_id, payload)).await;
                });
            },
            request_event::Payload::RunActionRequest(request) => {
                let handlers = Arc::clone(&self.handlers);
                tokio::spawn(async move {
                    let joined = tokio::task::spawn_blocking(move || {
                        match handler(&handlers, capability_id) {
                            Ok(capability) => match capability.search_handler() {
                                Some(search) => match request.action {
                                    Some(action) => response_event::Payload::RunActionResponse(
                                        RunActionResponse {
                                            behavior: Some(search.run_search_action(action)),
                                        },
                                    ),
                                    None => error_payload("run_action request has no action"),
                                },
                                None => error_payload(&format!(
                                    "capability {} does not implement search handler",
                                    capability.id()
                                )),
                            },
                            Err(error) => error_payload(&error),
                        }
                    })
                    .await;

                    let payload = match joined {
                        Ok(payload) => payload,
                        Err(e) => error_payload(&format!("run_action handler panicked: {e}")),
                    };
                    let _ = tx.send(response(event_id, payload)).await;
                });
            },
            request_event::Payload::InvokeToolRequest(request) => {
                let handlers = Arc::clone(&self.handlers);
                let cancellation = Arc::clone(&self.cancellation);

                let token = cancellation
                    .entry(request.session_id.clone())
                    .or_default()
                    .clone();

                tokio::spawn(async move {
                    let payload = match handler(&handlers, capability_id) {
                        Ok(capability) => match capability.tool_handler() {
                            Some(tool) => {
                                let outcome = tokio::select! {
                                    outcome = tool.invoke(
                                        &request.session_id,
                                        &request.call_id,
                                        &request.arguments,
                                    ) => outcome,
                                    _ = token.cancelled() => Err("tool invocation cancelled".into()),
                                };
                                match outcome {
                                    Ok(content) => response_event::Payload::InvokeToolResponse(
                                        InvokeToolResponse {
                                            content: Some(content),
                                        },
                                    ),
                                    Err(error) => error_payload(&error),
                                }
                            },
                            None => error_payload(&format!(
                                "capability {} does not implement tool handler",
                                capability.id()
                            )),
                        },
                        Err(error) => error_payload(&error),
                    };

                    let _ = tx.send(response(event_id, payload)).await;
                });
            },
            request_event::Payload::CancelToolRequest(request) => {
                if let Some((_, token)) = self.cancellation.remove(&request.session_id) {
                    token.cancel();
                }
                let handlers = Arc::clone(&self.handlers);
                tokio::spawn(async move {
                    let mut errors = Vec::new();
                    for capability in handlers.values() {
                        if let Some(tool) = capability.tool_handler()
                            && let Err(error) = tool.cancel(&request.session_id).await
                        {
                            errors.push(format!("{}: {error}", capability.id()));
                        }
                    }
                    let payload = if errors.is_empty() {
                        response_event::Payload::CancelToolResponse(CancelToolResponse {})
                    } else {
                        error_payload(&errors.join("; "))
                    };
                    let _ = tx.send(response(event_id, payload)).await;
                });
            },
        }
    }
}

fn handler(
    handlers: &HashMap<String, Box<dyn Capability>>,
    capability_id: Option<String>,
) -> Result<&dyn Capability, String> {
    capability_id
        .as_deref()
        .and_then(|id| handlers.get(id))
        .map(|handler| handler.as_ref())
        .ok_or_else(|| {
            format!(
                "unknown capability: {}",
                capability_id.as_deref().unwrap_or("<missing>")
            )
        })
}

fn response(event_id: u64, payload: response_event::Payload) -> ResponseEvent {
    ResponseEvent {
        event_id,
        payload: Some(payload),
    }
}

fn error_payload(error: &str) -> response_event::Payload {
    response_event::Payload::ExtensionError(ExtensionError {
        error: error.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use paloma_extension_protocol::v1::{
        Action, CancelToolRequest, HandshakeRequest, Hide, InvokeToolRequest, Item,
        RunActionRequest, SearchRequest, Stay, ToolContent, ToolFacet,
        run_action_response::Behavior,
    };
    use tokio::sync::Notify;

    use super::*;
    use crate::{Capability, SearchHandler, ToolHandler};

    struct SearchOnly(&'static str);

    impl Capability for SearchOnly {
        fn id(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "stub capability"
        }

        fn search_handler(&self) -> Option<&dyn SearchHandler> {
            Some(self)
        }
    }

    impl SearchHandler for SearchOnly {
        fn search(&self, input: &str) -> Vec<Item> {
            vec![Item {
                title: format!("{}: {input}", self.0),
                subtitle: None,
                icon: None,
                actions: vec![],
            }]
        }

        fn run_search_action(&self, action: Action) -> Behavior {
            match action.label.as_str() {
                "hide" => Behavior::Hide(Hide {}),
                _ => Behavior::Stay(Stay {}),
            }
        }
    }

    struct ToolOnly;

    impl Capability for ToolOnly {
        fn id(&self) -> &str {
            "tool"
        }

        fn description(&self) -> &str {
            "tool only capability"
        }

        fn tool_handler(&self) -> Option<&dyn ToolHandler> {
            Some(self)
        }
    }

    #[async_trait]
    impl ToolHandler for ToolOnly {
        fn facet(&self) -> ToolFacet {
            ToolFacet {
                description: "tool only facet".into(),
                short_description: "tool only facet".into(),
                parameters: "{}".into(),
            }
        }

        async fn invoke(
            &self,
            _session_id: &str,
            call_id: &str,
            arguments: &str,
        ) -> Result<ToolContent, String> {
            Ok(ToolContent::text(format!("{call_id}: {arguments}")))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    struct CancelOnly {
        started: Arc<Notify>,
        cancelled: Arc<AtomicBool>,
    }

    impl Capability for CancelOnly {
        fn id(&self) -> &str {
            "cancel"
        }

        fn description(&self) -> &str {
            "cancel only capability"
        }

        fn tool_handler(&self) -> Option<&dyn ToolHandler> {
            Some(self)
        }
    }

    #[async_trait]
    impl ToolHandler for CancelOnly {
        fn facet(&self) -> ToolFacet {
            ToolFacet {
                description: "cancel only facet".into(),
                short_description: "cancel only facet".into(),
                parameters: "{}".into(),
            }
        }

        async fn invoke(
            &self,
            _session_id: &str,
            _call_id: &str,
            _arguments: &str,
        ) -> Result<ToolContent, String> {
            self.started.notify_one();
            std::future::pending().await
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), String> {
            self.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn service() -> ExtensionService {
        ExtensionService::new(
            "Test",
            "test extension.",
            Some("author".into()),
            None,
            vec![Box::new(SearchOnly("beta")), Box::new(SearchOnly("alpha"))],
        )
    }

    fn tool_service() -> ExtensionService {
        ExtensionService::new(
            "Test",
            "test extension.",
            None,
            None,
            vec![Box::new(SearchOnly("alpha")), Box::new(ToolOnly)],
        )
    }

    async fn roundtrip(service: &ExtensionService, request: RequestEvent) -> ResponseEvent {
        let (tx, mut rx) = mpsc::channel(1);
        service.handle(request, tx).await;
        rx.recv().await.expect("no response event")
    }

    fn extension_error(response: ResponseEvent) -> String {
        match response.payload {
            Some(response_event::Payload::ExtensionError(e)) => e.error,
            other => panic!("expected an extension error, got {other:?}"),
        }
    }

    struct Panicking;

    impl Capability for Panicking {
        fn id(&self) -> &str {
            "boom"
        }

        fn description(&self) -> &str {
            "panicking capability"
        }

        fn search_handler(&self) -> Option<&dyn SearchHandler> {
            Some(self)
        }
    }

    impl SearchHandler for Panicking {
        fn search(&self, _input: &str) -> Vec<Item> {
            panic!("search exploded");
        }

        fn run_search_action(&self, _action: Action) -> Behavior {
            panic!("run_action exploded");
        }
    }

    fn panicking_service() -> ExtensionService {
        ExtensionService::new(
            "Test",
            "test extension.",
            None,
            None,
            vec![Box::new(Panicking)],
        )
    }

    #[tokio::test]
    async fn panicking_search_handler_reports_an_error() {
        let response = roundtrip(
            &panicking_service(),
            RequestEvent {
                event_id: 21,
                capability_id: Some("boom".into()),
                payload: Some(request_event::Payload::SearchRequest(SearchRequest {
                    input: "hi".into(),
                })),
            },
        )
        .await;

        assert_eq!(response.event_id, 21);
        assert!(
            extension_error(response).contains("search handler panicked"),
            "panic should surface as an extension error"
        );
    }

    #[tokio::test]
    async fn panicking_run_action_handler_reports_an_error() {
        let response = roundtrip(
            &panicking_service(),
            RequestEvent {
                event_id: 22,
                capability_id: Some("boom".into()),
                payload: Some(request_event::Payload::RunActionRequest(RunActionRequest {
                    action: Some(Action {
                        label: "hide".into(),
                        params: vec![],
                        primary: true,
                    }),
                })),
            },
        )
        .await;

        assert_eq!(response.event_id, 22);
        assert!(
            extension_error(response).contains("run_action handler panicked"),
            "panic should surface as an extension error"
        );
    }

    #[tokio::test]
    async fn handshake_reports_metadata_with_sorted_capabilities() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 7,
                capability_id: None,
                payload: Some(request_event::Payload::HandshakeRequest(
                    HandshakeRequest {},
                )),
            },
        )
        .await;

        assert_eq!(response.event_id, 7);
        let Some(response_event::Payload::HandshakeResponse(handshake)) = response.payload else {
            panic!("expected a handshake response");
        };
        assert_eq!(handshake.version, PROTOCOL_VERSION);
        assert_eq!(handshake.extension_id, "Test");
        assert_eq!(handshake.description, "test extension.");
        assert_eq!(handshake.author.as_deref(), Some("author"));
        assert_eq!(handshake.homepage, None);
        let ids: Vec<_> = handshake
            .capabilities
            .iter()
            .map(|capability| capability.capability_id.as_str())
            .collect();
        assert_eq!(ids, ["alpha", "beta"]);
        assert!(
            handshake
                .capabilities
                .iter()
                .all(|capability| capability.search.is_some() && capability.tool.is_none())
        );
    }

    #[tokio::test]
    async fn search_routes_by_capability_id() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 3,
                capability_id: Some("alpha".into()),
                payload: Some(request_event::Payload::SearchRequest(SearchRequest {
                    input: "query".into(),
                })),
            },
        )
        .await;

        assert_eq!(response.event_id, 3);
        let Some(response_event::Payload::SearchResponse(search)) = response.payload else {
            panic!("expected a search response");
        };
        assert_eq!(search.items.len(), 1);
        assert_eq!(search.items[0].title, "alpha: query");
    }

    #[tokio::test]
    async fn search_unknown_capability_errors() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 4,
                capability_id: Some("gamma".into()),
                payload: Some(request_event::Payload::SearchRequest(SearchRequest {
                    input: "query".into(),
                })),
            },
        )
        .await;

        assert_eq!(response.event_id, 4);
        assert_eq!(extension_error(response), "unknown capability: gamma");
    }

    #[tokio::test]
    async fn search_missing_capability_id_errors() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 5,
                capability_id: None,
                payload: Some(request_event::Payload::SearchRequest(SearchRequest {
                    input: "query".into(),
                })),
            },
        )
        .await;

        assert_eq!(extension_error(response), "unknown capability: <missing>");
    }

    #[tokio::test]
    async fn missing_payload_errors() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 6,
                capability_id: Some("alpha".into()),
                payload: None,
            },
        )
        .await;

        assert_eq!(response.event_id, 6);
        assert_eq!(
            extension_error(response),
            "unsupported or missing request payload"
        );
    }

    #[tokio::test]
    async fn run_action_returns_behavior() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 8,
                capability_id: Some("beta".into()),
                payload: Some(request_event::Payload::RunActionRequest(RunActionRequest {
                    action: Some(Action {
                        label: "hide".into(),
                        params: vec![],
                        primary: true,
                    }),
                })),
            },
        )
        .await;

        assert_eq!(response.event_id, 8);
        let Some(response_event::Payload::RunActionResponse(run)) = response.payload else {
            panic!("expected a run action response");
        };
        assert_eq!(run.behavior, Some(Behavior::Hide(Hide {})));
    }

    #[tokio::test]
    async fn run_action_without_action_errors() {
        let response = roundtrip(
            &service(),
            RequestEvent {
                event_id: 9,
                capability_id: Some("beta".into()),
                payload: Some(request_event::Payload::RunActionRequest(RunActionRequest {
                    action: None,
                })),
            },
        )
        .await;

        assert_eq!(
            extension_error(response),
            "run_action request has no action"
        );
    }

    #[tokio::test]
    async fn search_without_search_handler_errors() {
        let response = roundtrip(
            &tool_service(),
            RequestEvent {
                event_id: 10,
                capability_id: Some("tool".into()),
                payload: Some(request_event::Payload::SearchRequest(SearchRequest {
                    input: "query".into(),
                })),
            },
        )
        .await;

        assert_eq!(
            extension_error(response),
            "capability tool does not implement search handler"
        );
    }

    #[tokio::test]
    async fn handshake_derives_tool_facet() {
        let response = roundtrip(
            &tool_service(),
            RequestEvent {
                event_id: 11,
                capability_id: None,
                payload: Some(request_event::Payload::HandshakeRequest(
                    HandshakeRequest {},
                )),
            },
        )
        .await;

        let Some(response_event::Payload::HandshakeResponse(handshake)) = response.payload else {
            panic!("expected a handshake response");
        };
        let tool_only = handshake
            .capabilities
            .iter()
            .find(|capability| capability.capability_id == "tool")
            .expect("tool only capability");
        assert!(tool_only.search.is_none());
        assert_eq!(
            tool_only.tool.as_ref().expect("tool facet").description,
            "tool only facet"
        );
    }

    #[tokio::test]
    async fn invoke_tool_returns_text() {
        let response = roundtrip(
            &tool_service(),
            RequestEvent {
                event_id: 12,
                capability_id: Some("tool".into()),
                payload: Some(request_event::Payload::InvokeToolRequest(
                    InvokeToolRequest {
                        session_id: "session".into(),
                        call_id: "call-1".into(),
                        arguments: "{\"a\":1}".into(),
                    },
                )),
            },
        )
        .await;

        assert_eq!(response.event_id, 12);
        let Some(response_event::Payload::InvokeToolResponse(invoke)) = response.payload else {
            panic!("expected an invoke tool response");
        };
        assert_eq!(invoke.content, Some(ToolContent::text("call-1: {\"a\":1}")));
    }

    #[tokio::test]
    async fn invoke_without_tool_handler_errors() {
        let response = roundtrip(
            &tool_service(),
            RequestEvent {
                event_id: 13,
                capability_id: Some("alpha".into()),
                payload: Some(request_event::Payload::InvokeToolRequest(
                    InvokeToolRequest {
                        session_id: "session".into(),
                        call_id: "call-1".into(),
                        arguments: "{}".into(),
                    },
                )),
            },
        )
        .await;

        assert_eq!(
            extension_error(response),
            "capability alpha does not implement tool handler"
        );
    }

    #[tokio::test]
    async fn cancel_unknown_session_acks() {
        let service = tool_service();
        let token = CancellationToken::new();
        service.cancellation.insert("other".into(), token.clone());

        let response = roundtrip(
            &service,
            RequestEvent {
                event_id: 14,
                capability_id: None,
                payload: Some(request_event::Payload::CancelToolRequest(
                    CancelToolRequest {
                        session_id: "nope".into(),
                    },
                )),
            },
        )
        .await;

        assert_eq!(response.event_id, 14);
        assert!(matches!(
            response.payload,
            Some(response_event::Payload::CancelToolResponse(_))
        ));
        assert!(!token.is_cancelled());
        assert!(service.cancellation.contains_key("other"));
        assert!(!service.cancellation.contains_key("nope"));
    }

    #[tokio::test]
    async fn cancel_interrupts_inflight_invoke() {
        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let service = ExtensionService::new(
            "Test",
            "test extension.",
            None,
            None,
            vec![Box::new(CancelOnly {
                started: Arc::clone(&started),
                cancelled: Arc::clone(&cancelled),
            })],
        );

        let (tx, mut rx) = mpsc::channel(2);
        service
            .handle(
                RequestEvent {
                    event_id: 15,
                    capability_id: Some("cancel".into()),
                    payload: Some(request_event::Payload::InvokeToolRequest(
                        InvokeToolRequest {
                            session_id: "session".into(),
                            call_id: "call-1".into(),
                            arguments: "{}".into(),
                        },
                    )),
                },
                tx.clone(),
            )
            .await;
        started.notified().await;

        service
            .handle(
                RequestEvent {
                    event_id: 16,
                    capability_id: None,
                    payload: Some(request_event::Payload::CancelToolRequest(
                        CancelToolRequest {
                            session_id: "session".into(),
                        },
                    )),
                },
                tx,
            )
            .await;

        for _ in 0..2 {
            let response = rx.recv().await.expect("missing response");
            match response.event_id {
                15 => assert_eq!(extension_error(response), "tool invocation cancelled"),
                16 => assert!(matches!(
                    response.payload,
                    Some(response_event::Payload::CancelToolResponse(_))
                )),
                other => panic!("unexpected event id {other}"),
            }
        }
        assert!(rx.recv().await.is_none());
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[test]
    #[should_panic(expected = "duplicate capability id: alpha")]
    fn duplicate_capability_id_panics() {
        ExtensionService::new(
            "Test",
            "test extension.",
            None,
            None,
            vec![Box::new(SearchOnly("alpha")), Box::new(SearchOnly("alpha"))],
        );
    }
}
