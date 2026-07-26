use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use dashmap::DashMap;
use scry_utils::{RefreshSlot, ttl_with_jitter};

use crate::entity::ToolSpec;

const MCP_SPECS_CACHE_TTL_SECS: u64 = Duration::from_hours(6).as_secs();
const MCP_SPECS_CACHE_JITTER_SECS: u64 = Duration::from_mins(30).as_secs();

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
        let ttl = ttl_with_jitter(MCP_SPECS_CACHE_TTL_SECS, MCP_SPECS_CACHE_JITTER_SECS, &id);
        let slot = self.specs.entry(id).or_default().clone();
        slot.get_or_refresh(ttl, fetch).await
    }

    pub async fn insert(&self, id: String, specs: HashMap<String, ToolSpec>) {
        let ttl = ttl_with_jitter(MCP_SPECS_CACHE_TTL_SECS, MCP_SPECS_CACHE_JITTER_SECS, &id);
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
