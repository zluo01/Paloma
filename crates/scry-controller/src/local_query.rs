use std::sync::Arc;

use dashmap::DashMap;
use tokio::task::JoinSet;

use scry_capability::native::app_search::AppSearch;
use scry_capability::native::clipboard::Clipboard;
use scry_capability::{Action, ActionOutcome, Item, QueryHandler};
use serde::Serialize;

pub struct LocalQuery {
    handlers: DashMap<&'static str, Arc<dyn QueryHandler>>,
}

pub struct QueryResponse {
    /// handler unique name
    pub id: &'static str,
    /// Display section name
    pub name: String,
    /// handler results
    pub items: Vec<Item>,
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

    pub async fn query(&self, input: &str) -> Vec<QueryResponse> {
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

        let mut responses = Vec::with_capacity(self.handlers.len());
        while let Some(joined) = set.join_next().await {
            if let Ok(response) = joined {
                responses.push(response);
            }
        }
        responses
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
