use std::{fmt::Display, future::Future, sync::Arc, time::Duration};

use dashmap::DashMap;
use scry_provider_protocol::v1::Model;
use scry_utils::RefreshSlot;

const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(8).as_secs();

pub struct ProviderCache {
    models: DashMap<String, Arc<RefreshSlot<Vec<Model>>>>,
    values: DashMap<String, Arc<RefreshSlot<String>>>,
}

impl ProviderCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            models: DashMap::new(),
            values: DashMap::new(),
        })
    }

    pub async fn value<E, F, Fut>(&self, key: &str, ttl_secs: u64, fetch: F) -> Result<String, E>
    where
        E: Display,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, E>>,
    {
        let slot = self.values.entry(key.to_owned()).or_default().clone();
        slot.get_or_refresh(ttl_secs, fetch).await
    }

    pub async fn models<E, F, Fut>(&self, id: String, fetch: F) -> Result<Vec<Model>, E>
    where
        E: Display,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<Model>, E>>,
    {
        let slot = self.models.entry(id).or_default().clone();
        slot.get_or_refresh(MODELS_CACHE_TTL_SECS, fetch).await
    }

    pub async fn insert_models(&self, id: String, models: Vec<Model>) {
        let slot = self.models.entry(id).or_default().clone();
        slot.insert(models, MODELS_CACHE_TTL_SECS).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn model(id: &str) -> Model {
        Model {
            id: id.into(),
            name: id.into(),
            default_reasoning_effort: String::new(),
            supported_reasoning_efforts: Vec::new(),
        }
    }

    #[tokio::test]
    async fn insert_models_seeds_the_read_through() {
        let cache = ProviderCache::new();
        cache.insert_models("codex".into(), vec![model("m1")]).await;

        let calls = AtomicUsize::new(0);
        let models = cache
            .models("codex".into(), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>(Vec::new())
            })
            .await
            .unwrap();
        assert_eq!(models[0].id, "m1");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn string_values_cache_by_key() {
        let cache = ProviderCache::new();
        let calls = AtomicUsize::new(0);
        for _ in 0..2 {
            let value = cache
                .value("a", 60, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, String>("one".to_string())
                })
                .await;
            assert_eq!(value.unwrap(), "one");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let other = cache
            .value("b", 60, || async { Ok::<_, String>("two".to_string()) })
            .await;
        assert_eq!(other.unwrap(), "two");
    }

    #[tokio::test]
    async fn cache_keys_are_independent() {
        let cache = ProviderCache::new();
        cache
            .insert_models("codex".into(), vec![model("codex")])
            .await;

        let fetched = cache
            .models("openai".into(), || async {
                Ok::<_, String>(vec![model("openai")])
            })
            .await
            .unwrap();
        assert_eq!(fetched[0].id, "openai");
    }
}
