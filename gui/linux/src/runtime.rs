//! Shared tokio runtime and GTK/tokio bridge helpers.
//!
//! GTK stays on the main loop. Network, storage, shortcut, and refresh work run
//! on one static multi-threaded tokio runtime. GTK signal handlers spawn async
//! work onto tokio, then await the result from the main context before touching
//! widgets again.

use std::{future::Future, sync::OnceLock};

use gtk4::glib;
use tokio::runtime::{Builder, Runtime};

/// Process-wide tokio runtime, initialized lazily on first use.
pub(crate) fn tokio_runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .thread_name("scry-tokio")
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// Run `work` on tokio and await its result from any executor.
///
/// GTK callers usually await this from `spawn_local`, so their continuation
/// returns to the main thread and can keep using widgets and `Rc`s.
///
/// Dropping the returned future does not abort `work`.
pub(crate) fn spawn<F, T>(work: F) -> impl Future<Output = T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = tokio_runtime().spawn(work);
    async move { handle.await.expect("scry-tokio task panicked") }
}

/// Spawn `work` on tokio and run `done` with its result on the GTK main
/// thread, where `done` may freely touch widgets.
pub(crate) fn spawn_with<F, T>(work: F, done: impl FnOnce(T) + 'static)
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    glib::MainContext::default().spawn_local(async move {
        done(spawn(work).await);
    });
}
