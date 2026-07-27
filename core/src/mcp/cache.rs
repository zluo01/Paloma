use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use dashmap::DashMap;
use scry_utils::{RefreshSlot, ttl_with_jitter};

use crate::entity::ToolSpec;

const MCP_SPECS_CACHE_TTL_SECS: u64 = Duration::from_hours(6).as_secs();
const MCP_SPECS_CACHE_JITTER_SECS: u64 = Duration::from_mins(30).as_secs();

pub struct McpToolSpecCache {
    specs: DashMap<String, RefreshSlot<HashMap<String, ToolSpec>>>,
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
        E: Display + Send + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<HashMap<String, ToolSpec>, E>> + Send + 'static,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ToolSchema;

    fn specs(tool: &str) -> HashMap<String, ToolSpec> {
        HashMap::from([(
            tool.to_string(),
            ToolSpec {
                name: "server".to_string(),
                tool: tool.to_string(),
                schema: ToolSchema {
                    name: tool.to_string(),
                    description: String::new(),
                    parameters: serde_json::Value::Null,
                },
            },
        )])
    }

    #[tokio::test]
    async fn fresh_specs_are_served_without_fetching() {
        let cache = McpToolSpecCache::new();
        cache.insert("server".into(), specs("old")).await;

        let served = cache
            .specs("server".into(), || async {
                Ok::<_, String>(specs("should-not-fetch"))
            })
            .await
            .unwrap();

        assert!(served.contains_key("old"));
        assert!(!served.contains_key("should-not-fetch"));
    }

    #[tokio::test]
    async fn expired_specs_serve_stale_and_refresh_in_background() {
        let cache = McpToolSpecCache::new();
        let slot = cache.specs.entry("server".into()).or_default().clone();
        slot.insert(specs("old"), 0).await;

        let served = cache
            .specs("server".into(), || async { Ok::<_, String>(specs("new")) })
            .await
            .unwrap();
        assert!(served.contains_key("old"), "should serve the stale value");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if cache.spec("server", "new").await.is_some() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("background refresh never landed");
    }
}
