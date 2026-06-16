// Shared tokio runtime + GTK ↔ tokio bridging helpers.
//
// One static multi-threaded tokio runtime services every async call
// the app makes. GTK runs on its own main loop on the main thread;
// tokio worker threads handle network, storage, portal, and refresh
// loops. Anything async that needs to be triggered from a GTK signal
// handler must be spawned onto tokio via `runtime::spawn`, then its
// result awaited inside `glib::MainContext::default().spawn_local`
// so the continuation runs back on the main thread before touching
// widgets.

use std::{future::Future, sync::OnceLock};

use gtk4::glib;
use tokio::runtime::{Builder, Runtime};

/// Process-wide tokio runtime. Initialised lazily on first use.
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

/// Spawn `work` on the shared tokio runtime and return a `Future`
/// for its result. The returned future is `Send`-agnostic: it can be
/// awaited from any executor — typically
/// `glib::MainContext::default().spawn_local` for GTK signal
/// handlers — so the continuation runs on the caller's thread and
/// may freely capture `Rc`/widgets.
///
/// `work` itself must be `Send + 'static` because it is moved onto
/// a tokio worker thread.
///
/// Cancel semantics match `tokio::spawn`: dropping the returned
/// future does **not** abort the spawned task. Panics inside the
/// task surface as a panic when the returned future is awaited.
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
