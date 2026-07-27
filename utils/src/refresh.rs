use std::{
    fmt::Display,
    future::Future,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use log::error;
use tokio::sync::Mutex;

use crate::unix_now;

/// add some variation to the cache TTL so we do not refresh everything at the same time.
pub fn ttl_with_jitter(base_secs: u64, jitter_secs: u64, key: &str) -> u64 {
    if jitter_secs == 0 {
        return base_secs;
    }
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    base_secs + hasher.finish() % jitter_secs
}

struct Timestamped<T> {
    value: T,
    expires_at: u64,
}

struct RefreshState<T> {
    cached: Option<Timestamped<T>>,
    revision: u64,
    refreshing: bool,
}

impl<T> RefreshState<T> {
    fn replace(&mut self, value: T, ttl_secs: u64) {
        self.revision = self.revision.wrapping_add(1);
        self.cached = Some(Timestamped {
            value,
            expires_at: unix_now() + ttl_secs,
        });
    }
}

impl<T> Default for RefreshState<T> {
    fn default() -> Self {
        Self {
            cached: None,
            revision: 0,
            refreshing: false,
        }
    }
}

pub struct RefreshSlot<T> {
    inner: Arc<Mutex<RefreshState<T>>>,
}

impl<T> Clone for RefreshSlot<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Default for RefreshSlot<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RefreshState::default())),
        }
    }
}

impl<T: Clone> RefreshSlot<T> {
    pub async fn peek(&self) -> Option<T> {
        self.inner
            .lock()
            .await
            .cached
            .as_ref()
            .map(|cached| cached.value.clone())
    }

    pub async fn insert(&self, value: T, ttl_secs: u64) {
        self.inner.lock().await.replace(value, ttl_secs);
    }

    pub async fn get_or_refresh<E, F, Fut>(&self, ttl_secs: u64, fetch: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: Display + Send + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
    {
        let mut state = self.inner.lock().await;

        if let Some(cached) = state.cached.as_ref() {
            let value = cached.value.clone();
            if unix_now() < cached.expires_at || state.refreshing {
                return Ok(value);
            }

            // Stale-while-revalidate
            state.refreshing = true;
            let revision = state.revision;
            let inner = Arc::clone(&self.inner);
            let pending = fetch();
            tokio::spawn(async move {
                let refreshed = pending.await;
                let mut state = inner.lock().await;
                state.refreshing = false;
                match refreshed {
                    Ok(fresh) if state.revision == revision => state.replace(fresh, ttl_secs),
                    Ok(_) => {},
                    Err(e) => error!("fail to refresh cache value. {}", e),
                }
            });
            return Ok(value);
        }

        let value = fetch().await?;
        state.replace(value.clone(), ttl_secs);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use tokio::sync::{Notify, oneshot};

    use super::*;

    struct DropSignalValue {
        value: u32,
        dropped: Option<Arc<Notify>>,
    }

    impl DropSignalValue {
        fn new(value: u32) -> Self {
            Self {
                value,
                dropped: None,
            }
        }

        fn tracked(value: u32, dropped: &Arc<Notify>) -> Self {
            Self {
                value,
                dropped: Some(Arc::clone(dropped)),
            }
        }
    }

    impl Clone for DropSignalValue {
        fn clone(&self) -> Self {
            Self::new(self.value)
        }
    }

    impl Drop for DropSignalValue {
        fn drop(&mut self) {
            if let Some(dropped) = &self.dropped {
                dropped.notify_one();
            }
        }
    }

    async fn await_signal(signal: &Notify, expectation: &str) {
        tokio::time::timeout(Duration::from_secs(1), signal.notified())
            .await
            .expect(expectation);
    }

    #[tokio::test]
    async fn fresh_value_is_served_without_fetching() {
        let slot = RefreshSlot::default();
        let calls = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let calls = Arc::clone(&calls);
            let value = slot
                .get_or_refresh(60, || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, String>(5)
                })
                .await;
            assert_eq!(value.unwrap(), 5);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_value_is_served_stale_then_refreshed() {
        let slot = RefreshSlot::default();
        let replaced = Arc::new(Notify::new());
        slot.insert(DropSignalValue::tracked(1, &replaced), 0).await;

        let stale = slot
            .get_or_refresh(60, || async { Ok::<_, String>(DropSignalValue::new(2)) })
            .await
            .unwrap();
        assert_eq!(stale.value, 1, "stale value is served immediately");

        await_signal(&replaced, "background refresh never landed").await;
        assert_eq!(slot.peek().await.unwrap().value, 2);
    }

    #[tokio::test]
    async fn failed_refresh_keeps_serving_stale_and_retries() {
        let slot = RefreshSlot::default();
        let replaced = Arc::new(Notify::new());
        slot.insert(DropSignalValue::tracked(1, &replaced), 0).await;

        let (failed_tx, failed_rx) = oneshot::channel();
        let stale = slot
            .get_or_refresh(60, || async move {
                failed_tx.send(()).unwrap();
                Err::<DropSignalValue, _>("down".to_string())
            })
            .await
            .unwrap();
        assert_eq!(stale.value, 1);
        failed_rx.await.unwrap();

        // the failed refresh must not re-stamp the TTL: this call refreshes again
        let stale = slot
            .get_or_refresh(60, || async { Ok::<_, String>(DropSignalValue::new(2)) })
            .await
            .unwrap();
        assert_eq!(stale.value, 1);

        await_signal(&replaced, "retry after a failed refresh never landed").await;
        assert_eq!(slot.peek().await.unwrap().value, 2);
    }

    #[tokio::test]
    async fn stale_callers_share_one_refresh() {
        let slot = RefreshSlot::default();
        slot.insert(1u32, 0).await;

        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        let stale = slot
            .get_or_refresh(60, {
                let calls = Arc::clone(&calls);
                move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        started_tx.send(()).unwrap();
                        release_rx.await.unwrap();
                        Ok::<_, String>(2u32)
                    }
                }
            })
            .await
            .unwrap();
        assert_eq!(stale, 1);

        started_rx.await.unwrap();

        for _ in 0..3 {
            let stale = slot
                .get_or_refresh(60, {
                    let calls = Arc::clone(&calls);
                    move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async move { Ok::<_, String>(99u32) }
                    }
                })
                .await
                .unwrap();
            assert_eq!(stale, 1);
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a refresh already in flight must not be duplicated"
        );

        release_tx.send(()).unwrap();
    }

    #[tokio::test]
    async fn insert_supersedes_older_background_refresh() {
        let slot = RefreshSlot::default();
        slot.insert(DropSignalValue::new(1), 0).await;

        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let discarded = Arc::new(Notify::new());
        let stale = slot
            .get_or_refresh(60, || {
                let discarded = Arc::clone(&discarded);
                async move {
                    started_tx.send(()).unwrap();
                    release_rx.await.unwrap();
                    Ok::<_, String>(DropSignalValue::tracked(2, &discarded))
                }
            })
            .await
            .unwrap();
        assert_eq!(stale.value, 1);

        started_rx.await.unwrap();
        slot.insert(DropSignalValue::new(3), 60).await;
        release_tx.send(()).unwrap();
        await_signal(&discarded, "older refresh result was not discarded").await;

        assert_eq!(slot.peek().await.unwrap().value, 3);
    }

    #[test]
    fn jitter_is_stable_and_bounded() {
        let a = ttl_with_jitter(100, 60, "github");
        assert_eq!(a, ttl_with_jitter(100, 60, "github"));
        assert!((100..160).contains(&a));
        assert_eq!(ttl_with_jitter(100, 0, "github"), 100);
    }

    #[tokio::test]
    async fn peek_serves_without_fetching_even_expired() {
        let slot = RefreshSlot::<u32>::default();
        assert_eq!(slot.peek().await, None);

        slot.insert(5, 0).await;
        assert_eq!(slot.peek().await, Some(5));
    }

    #[tokio::test]
    async fn cold_slot_error_propagates() {
        let slot = RefreshSlot::<u32>::default();
        let result = slot
            .get_or_refresh(60, || async { Err::<u32, _>("boom".to_string()) })
            .await;
        assert_eq!(result.unwrap_err(), "boom");
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_cold_callers_share_one_fetch() {
        let slot = RefreshSlot::<u32>::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let slot = slot.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                slot.get_or_refresh(60, || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok::<_, String>(7)
                    }
                })
                .await
            }));
        }
        for handle in handles {
            assert_eq!(handle.await.unwrap().unwrap(), 7);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
