use std::{fmt::Display, future::Future, sync::Arc, time::Duration};

use dashmap::DashMap;
use paloma_provider_protocol::v1::Model;
use paloma_utils::{RefreshSlot, ttl_with_jitter};

const MODELS_CACHE_TTL_SECS: u64 = Duration::from_hours(8).as_secs();
const MODELS_CACHE_JITTER_SECS: u64 = Duration::from_mins(30).as_secs();

pub struct ProviderCache {
    models: DashMap<String, RefreshSlot<Vec<Model>>>,
    values: DashMap<String, RefreshSlot<String>>,
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
        E: Display + Send + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, E>> + Send + 'static,
    {
        let slot = self.values.entry(key.to_owned()).or_default().clone();
        slot.get_or_refresh(ttl_secs, fetch).await
    }

    pub async fn models<E, F, Fut>(&self, id: String, fetch: F) -> Result<Vec<Model>, E>
    where
        E: Display + Send + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<Model>, E>> + Send + 'static,
    {
        let ttl = ttl_with_jitter(MODELS_CACHE_TTL_SECS, MODELS_CACHE_JITTER_SECS, &id);
        let slot = self.models.entry(id).or_default().clone();
        slot.get_or_refresh(ttl, fetch).await
    }

    pub async fn insert_models(&self, id: String, models: Vec<Model>) {
        let ttl = ttl_with_jitter(MODELS_CACHE_TTL_SECS, MODELS_CACHE_JITTER_SECS, &id);
        let slot = self.models.entry(id).or_default().clone();
        slot.insert(models, ttl).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> Model {
        Model {
            id: id.into(),
            name: id.into(),
            default_reasoning_effort: String::new(),
            supported_reasoning_efforts: Vec::new(),
        }
    }

    fn never_fetch<T>() -> std::future::Ready<Result<T, String>> {
        panic!("cache must not fetch")
    }

    #[tokio::test]
    async fn insert_models_seeds_the_read_through() {
        let cache = ProviderCache::new();
        cache.insert_models("codex".into(), vec![model("m1")]).await;

        let models = cache.models("codex".into(), never_fetch).await.unwrap();
        assert_eq!(models[0].id, "m1");
    }

    #[tokio::test]
    async fn string_values_cache_by_key() {
        let cache = ProviderCache::new();
        let first = cache
            .value("a", 60, || async { Ok::<_, String>("one".into()) })
            .await
            .unwrap();
        let cached = cache.value("a", 60, never_fetch).await.unwrap();

        let other = cache
            .value("b", 60, || async { Ok::<_, String>("two".into()) })
            .await
            .unwrap();

        assert_eq!(first, "one");
        assert_eq!(cached, "one");
        assert_eq!(other, "two");
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
