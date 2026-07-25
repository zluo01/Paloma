use std::{collections::HashMap, io, sync::Arc};

use scry_extension_protocol::{
    PROTOCOL_VERSION,
    v1::{
        Capability as CapabilityMeta, ExtensionError, HandshakeResponse, RequestEvent,
        ResponseEvent, RunActionResponse, SearchFacet, SearchResponse, request_event,
        response_event,
    },
};
use scry_utils::transport::serve_plugin;
use tokio::sync::mpsc;

use crate::Capability;

const EXTENSION_INNER_CHANNEL_CAPACITY: usize = 32;

pub struct ExtensionService {
    extension_id: String,
    description: String,
    author: Option<String>,
    homepage: Option<String>,
    handlers: Arc<HashMap<String, Box<dyn Capability>>>,
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
            let capability_id = handler.id().to_string();
            let previous = handlers.insert(capability_id.clone(), handler);
            assert!(
                previous.is_none(),
                "duplicate capability id: {capability_id}"
            );
        }
        Self {
            extension_id: extension_id.into(),
            description: description.into(),
            author,
            homepage,
            handlers: Arc::new(handlers),
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
                tokio::task::spawn_blocking(move || {
                    let payload = match handler(&handlers, capability_id) {
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
                    };
                    let _ = tx.blocking_send(response(event_id, payload));
                });
            },
            request_event::Payload::RunActionRequest(request) => {
                let handlers = Arc::clone(&self.handlers);
                tokio::task::spawn_blocking(move || {
                    let payload = match handler(&handlers, capability_id) {
                        Ok(capability) => match capability.search_handler() {
                            Some(search) => match request.action {
                                Some(action) => {
                                    response_event::Payload::RunActionResponse(RunActionResponse {
                                        behavior: Some(search.run_search_action(action)),
                                    })
                                },
                                None => error_payload("run_action request has no action"),
                            },
                            None => error_payload(&format!(
                                "capability {} does not implement search handler",
                                capability.id()
                            )),
                        },
                        Err(error) => error_payload(&error),
                    };
                    let _ = tx.blocking_send(response(event_id, payload));
                });
            },
            request_event::Payload::InvokeToolRequest(_) => {},
            request_event::Payload::CancelToolRequest(_) => {},
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
    use scry_extension_protocol::v1::{
        Action, HandshakeRequest, Hide, Item, RunActionRequest, SearchRequest, Stay,
        run_action_response::Behavior,
    };

    use super::*;
    use crate::{Capability, SearchHandler, ToolHandler};

    struct Stub(&'static str);

    impl Capability for Stub {
        fn id(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "stub capability"
        }

        fn search_handler(&self) -> Option<&dyn SearchHandler> {
            Some(self)
        }

        fn tool_handler(&self) -> Option<&dyn ToolHandler> {
            None
        }
    }

    impl SearchHandler for Stub {
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

    fn service() -> ExtensionService {
        ExtensionService::new(
            "Test",
            "test extension.",
            Some("author".into()),
            None,
            vec![Box::new(Stub("beta")), Box::new(Stub("alpha"))],
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

    #[test]
    #[should_panic(expected = "duplicate capability id: alpha")]
    fn duplicate_capability_id_panics() {
        ExtensionService::new(
            "Test",
            "test extension.",
            None,
            None,
            vec![Box::new(Stub("alpha")), Box::new(Stub("alpha"))],
        );
    }
}
