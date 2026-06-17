use std::sync::Arc;

use dashmap::DashMap;
use futures::{Stream, stream};
use log::error;
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    capability::{Action, ActionOutcome, AppSearch, Clipboard, QueryHandler},
    constants::RENDER_CHANNEL_CAPACITY,
    controller::{LocalRenderEvent, RenderEvent, entity::QueryResponse},
};

pub struct LocalQuery {
    handlers: DashMap<&'static str, Arc<dyn QueryHandler>>,
}

impl LocalQuery {
    pub fn new() -> Result<Self, LocalQueryInitError> {
        let handlers: DashMap<&'static str, Arc<dyn QueryHandler>> = DashMap::new();

        let app_search: Arc<dyn QueryHandler> =
            Arc::new(AppSearch::new().map_err(|source| LocalQueryInitError {
                handler: "app_search",
                source: source.to_string(),
            })?);
        handlers.insert(app_search.id(), app_search);

        let clipboard: Arc<dyn QueryHandler> = Arc::new(Clipboard::new());
        handlers.insert(clipboard.id(), clipboard);

        Ok(Self { handlers })
    }

    pub fn query(&self, input: &str) -> impl Stream<Item = RenderEvent> + use<> {
        let (render_tx, mut render_rx) = mpsc::channel(RENDER_CHANNEL_CAPACITY);

        let handlers: Vec<Arc<dyn QueryHandler>> = self
            .handlers
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect();
        let input = input.to_owned();

        tokio::spawn(async move {
            if input.trim().is_empty() {
                let _ = render_tx.send(RenderEvent::Done).await;
                return;
            }

            let mut set = JoinSet::new();
            for handler in handlers {
                let input = input.clone();
                set.spawn_blocking(move || QueryResponse {
                    id: handler.id(),
                    name: handler.metadata().name,
                    items: handler.query(&input),
                });
            }

            while let Some(joined) = set.join_next().await {
                let event = match joined {
                    Ok(response) => RenderEvent::Local(LocalRenderEvent::Append { response }),
                    Err(err) => RenderEvent::Error {
                        message: err.to_string(),
                    },
                };
                if render_tx.send(event).await.is_err() {
                    error!("failed to send render response.");
                    return;
                }
            }

            if render_tx.send(RenderEvent::Done).await.is_err() {
                error!("failed to send done event.");
            }
        });

        stream::poll_fn(move |cx| render_rx.poll_recv(cx))
    }

    pub fn run(&self, id: &str, action: Action) -> Option<ActionOutcome> {
        self.handlers.get(id).map(|handler| handler.run(action))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalQueryInitError {
    handler: &'static str,
    source: String,
}

impl std::fmt::Display for LocalQueryInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "failed to initialize native query handler {}: {}",
            self.handler, self.source
        )
    }
}

impl std::error::Error for LocalQueryInitError {}
