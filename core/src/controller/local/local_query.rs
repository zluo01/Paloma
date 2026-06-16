use std::sync::Arc;

use dashmap::DashMap;
use log::error;
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    capability::{Action, ActionOutcome, AppSearch, Clipboard, QueryHandler},
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

    pub async fn query(&self, input: &str, render_tx: mpsc::Sender<RenderEvent>) {
        if input.trim().is_empty() {
            if render_tx.send(RenderEvent::Done).await.is_err() {
                error!("local query: failed to send done event for empty input");
            }
            return;
        }

        let mut set = JoinSet::new();
        for entry in self.handlers.iter() {
            let id = *entry.key();
            let name = entry.value().metadata().name;
            let handler = Arc::clone(entry.value());
            let input = input.to_owned();
            set.spawn_blocking(move || QueryResponse {
                id,
                name,
                items: handler.query(&input),
            });
        }

        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(response) => {
                    let id = response.id;
                    if render_tx
                        .send(RenderEvent::Local(LocalRenderEvent::Append { response }))
                        .await
                        .is_err()
                    {
                        error!("local query: failed to send render response for handler {id}");
                        return;
                    }
                },
                Err(err) => {
                    let message = err.to_string();
                    if render_tx
                        .send(RenderEvent::Error {
                            message: message.clone(),
                        })
                        .await
                        .is_err()
                    {
                        error!("local query: failed to send join error to renderer: {message}");
                    }
                },
            }
        }

        if render_tx.send(RenderEvent::Done).await.is_err() {
            error!("local query: failed to send done event to renderer");
        }
    }

    pub fn run(&self, id: String, action: Action) -> Option<ActionOutcome> {
        self.handlers
            .get(id.as_str())
            .map(|handler| handler.run(action))
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
