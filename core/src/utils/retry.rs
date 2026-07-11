use std::future::Future;

use backon::{ExponentialBuilder, Retryable};

const MAX_RETRY_TIMES: usize = 3;

pub(crate) async fn attempt_with_retry<T, E, Fut>(
    call: impl Fn() -> Fut,
    retryable: impl FnMut(&E) -> bool,
) -> Result<T, E>
where
    Fut: Future<Output = Result<T, E>>,
{
    call.retry(
        ExponentialBuilder::default()
            .with_max_times(MAX_RETRY_TIMES)
            .with_jitter(),
    )
    .when(retryable)
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{MAX_RETRY_TIMES, attempt_with_retry};

    #[tokio::test(start_paused = true)]
    async fn first_success_needs_no_retry() {
        let calls = AtomicUsize::new(0);
        let result = attempt_with_retry(
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>(9)
            },
            |_| true,
        )
        .await;
        assert_eq!(result.unwrap(), 9);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_failures_retry_until_success() {
        let calls = AtomicUsize::new(0);
        let result = attempt_with_retry(
            || {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    if attempt < 2 {
                        Err("transient".to_string())
                    } else {
                        Ok(3)
                    }
                }
            },
            |_| true,
        )
        .await;
        assert_eq!(result.unwrap(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn non_retryable_error_fails_once() {
        let calls = AtomicUsize::new(0);
        let result: Result<u32, String> = attempt_with_retry(
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err("fatal".to_string())
            },
            |_| false,
        )
        .await;
        assert_eq!(result.unwrap_err(), "fatal");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_retry_budget() {
        let calls = AtomicUsize::new(0);
        let result: Result<u32, String> = attempt_with_retry(
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err("down".to_string())
            },
            |_| true,
        )
        .await;
        assert_eq!(result.unwrap_err(), "down");
        assert_eq!(calls.load(Ordering::SeqCst), MAX_RETRY_TIMES + 1);
    }
}
