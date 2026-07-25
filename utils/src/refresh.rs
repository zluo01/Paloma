use std::{fmt::Display, future::Future};

use log::error;
use tokio::sync::Mutex;

use crate::unix_now;

struct Timestamped<T> {
    value: T,
    expires_at: u64,
}

pub struct RefreshSlot<T> {
    inner: Mutex<Option<Timestamped<T>>>,
}

impl<T> Default for RefreshSlot<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl<T: Clone> RefreshSlot<T> {
    pub async fn insert(&self, value: T, ttl_secs: u64) {
        *self.inner.lock().await = Some(Timestamped {
            value,
            expires_at: unix_now() + ttl_secs,
        });
    }

    pub async fn get_or_refresh<E, F, Fut>(&self, ttl_secs: u64, fetch: F) -> Result<T, E>
    where
        E: Display,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut slot = self.inner.lock().await;
        if let Some(cached) = slot.as_ref()
            && unix_now() < cached.expires_at
        {
            return Ok(cached.value.clone());
        }
        match fetch().await {
            Ok(value) => {
                *slot = Some(Timestamped {
                    value: value.clone(),
                    expires_at: unix_now() + ttl_secs,
                });
                Ok(value)
            },
            // fetch failed: serve the stale value; expires_at stays in the
            // past, so the next call retries.
            Err(e) => {
                error!("fail to refresh cache value. {}", e);
                slot.as_ref().map(|cached| cached.value.clone()).ok_or(e)
            },
        }
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

    use super::*;

    #[tokio::test]
    async fn fresh_value_is_served_without_fetching() {
        let slot = RefreshSlot::default();
        let calls = AtomicUsize::new(0);
        for _ in 0..3 {
            let value = slot
                .get_or_refresh(60, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, String>(5)
                })
                .await;
            assert_eq!(value.unwrap(), 5);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_value_is_refetched() {
        let slot = RefreshSlot::default();
        let first = slot
            .get_or_refresh(0, || async { Ok::<_, String>(1) })
            .await;
        assert_eq!(first.unwrap(), 1);
        let second = slot
            .get_or_refresh(0, || async { Ok::<_, String>(2) })
            .await;
        assert_eq!(second.unwrap(), 2);
    }

    #[tokio::test]
    async fn failed_refresh_serves_stale_then_retries() {
        let slot = RefreshSlot::default();
        let seeded = slot
            .get_or_refresh(0, || async { Ok::<_, String>(1) })
            .await;
        assert_eq!(seeded.unwrap(), 1);

        let stale = slot
            .get_or_refresh(0, || async { Err::<u32, _>("down".to_string()) })
            .await;
        assert_eq!(stale.unwrap(), 1);

        // the stale serve must not re-stamp the TTL: this call fetches again
        let recovered = slot
            .get_or_refresh(60, || async { Ok::<_, String>(2) })
            .await;
        assert_eq!(recovered.unwrap(), 2);
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
    async fn concurrent_callers_share_one_fetch() {
        let slot = Arc::new(RefreshSlot::<u32>::default());
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
