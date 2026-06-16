//! GTK4/Linux frontend binary.
//!
//! One binary, one process: GTK on the main thread, tokio on a static
//! multi-threaded runtime, controllers shared via `Arc<AppContext>`.
//! `app` owns the bootstrap; `services` runs the portal/tray on tokio;
//! GTK-facing behavior is coordinated by `overlay::OverlayController`.

mod app;
mod runtime;
mod services;
mod style;
mod widgets;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
