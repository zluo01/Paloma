use std::{
    collections::HashMap,
    fmt::Display,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use dashmap::DashMap;
use scry_utils::RefreshSlot;

use crate::capability::ToolSpec;

const MCP_SPECS_CACHE_TTL_SECS: u64 = Duration::from_hours(6).as_secs();
const MCP_SPECS_CACHE_JITTER_SECS: u64 = Duration::from_mins(30).as_secs();

/// add some variation to the cache TTL so we do not refresh everything at the same time.
fn ttl_with_jitter(id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    MCP_SPECS_CACHE_TTL_SECS + hasher.finish() % MCP_SPECS_CACHE_JITTER_SECS
}

pub struct McpToolSpecCache {
    specs: DashMap<String, Arc<RefreshSlot<HashMap<String, ToolSpec>>>>,
}

impl McpToolSpecCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            specs: DashMap::new(),
        })
    }

    pub async fn specs<E, F, Fut>(
        &self,
        id: String,
        fetch: F,
    ) -> Result<HashMap<String, ToolSpec>, E>
    where
        E: Display,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<HashMap<String, ToolSpec>, E>>,
    {
        let ttl = ttl_with_jitter(&id);
        let slot = self.specs.entry(id).or_default().clone();
        slot.get_or_refresh(ttl, fetch).await
    }

    pub async fn insert(&self, id: String, specs: HashMap<String, ToolSpec>) {
        let ttl = ttl_with_jitter(&id);
        let slot = self.specs.entry(id).or_default().clone();
        slot.insert(specs, ttl).await;
    }

    pub async fn spec(&self, id: &str, name: &str) -> Option<ToolSpec> {
        let slot = self.specs.get(id)?.clone();
        slot.peek().await?.get(name).cloned()
    }

    pub fn remove(&self, id: &str) {
        self.specs.remove(id);
    }
}
