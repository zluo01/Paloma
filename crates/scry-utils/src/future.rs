use futures::future::Shared;
use futures::FutureExt;
use std::future::Future;
use tokio::sync::oneshot;

/// A simple wrapper on the oneshot to make it like completableFuture in Java
/// This will be used within mpsc so thread-safe is not a concern.
pub struct CompletableFuture<T: Clone> {
    read: Shared<oneshot::Receiver<T>>,
    complete: Option<oneshot::Sender<T>>,
}

impl<T: Clone> CompletableFuture<T> {
    pub fn pending() -> Self {
        let (tx, rx) = oneshot::channel();
        Self {
            read: rx.shared(),
            complete: Some(tx),
        }
    }

    pub fn completed(value: T) -> Self {
        let mut cf = Self::pending();
        cf.complete(value);
        cf
    }

    /// Complete the future with `value`. Returns `false` if it was already
    /// completed — the existing value is kept and `value` is dropped.
    pub fn complete(&mut self, value: T) -> bool {
        match self.complete.take() {
            Some(tx) => {
                let _ = tx.send(value);
                true
            }
            None => false,
        }
    }

    pub fn is_completed(&self) -> bool {
        self.complete.is_none()
    }

    pub fn get(&self) -> impl Future<Output = Option<T>> {
        let read = self.read.clone();
        async move { read.await.ok() }
    }
}
