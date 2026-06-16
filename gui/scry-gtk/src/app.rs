//! Single-process bootstrap.
//!
//! One binary, one process: GTK on the main thread, tokio on a static
//! multi-threaded runtime, controllers shared via `Arc<AppContext>`.
//! Portal/tray services run on tokio; GTK-facing behavior is coordinated
//! by `overlay::OverlayController`.

use std::{cell::RefCell, process::ExitCode, rc::Rc, sync::Arc};

use gtk4::{gio, glib, prelude::*};
use libadwaita::{Application, ApplicationWindow};
use log::{error, warn};
use scry_core::{AppContext, TrayEvent};
use tokio::sync::broadcast::error::RecvError;

use crate::{
    runtime::tokio_runtime,
    services, style,
    widgets::{overlay, settings},
};

const APP_ID: &str = "dev.scry.Scry";

type OpenSettingsFn = Rc<dyn Fn()>;

/// Initialise the global logger. Idempotent — `try_init` is a no-op if a
/// logger is already installed.
fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .try_init();
}

/// Boot the daemon-and-UI-in-one process. Returns when the user picks
/// "Quit" from the tray menu (or otherwise closes the GApplication).
pub(crate) fn run() -> ExitCode {
    init_logging();

    // Build the shared app state on the static tokio runtime, so any
    // tasks it spawns (refresh loops) live on the same runtime that
    // services subsequent runtime::spawn calls.
    let app_state = match tokio_runtime().block_on(AppContext::build()) {
        Ok(a) => a,
        Err(e) => {
            error!("startup: {e}");
            return ExitCode::from(1);
        },
    };

    // Background services. Both fan their events out via broadcast
    // channels; GTK subscribers are installed during activation.
    tokio_runtime().spawn({
        let hotkey = app_state.hotkey.clone();
        async move {
            if let Err(e) = services::portal::run(hotkey).await {
                warn!("portal: exited with error: {e}");
            }
        }
    });
    tokio_runtime().spawn({
        let events = app_state.tray_events.clone();
        async move { services::tray::run(events).await }
    });

    // AdwApplication initialises libadwaita in `startup`, before any window
    // is built — the overlay uses adw widgets, so that ordering matters.
    let gapp = Application::builder()
        .application_id(APP_ID)
        // NON_UNIQUE: a launcher binary should never refuse to start
        // because a stale instance is registered. We dedupe via the
        // hotkey/tray model, not GApplication uniqueness.
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let app_for_activate = app_state.clone();
    gapp.connect_activate(move |g| on_activate(g, app_for_activate.clone()));

    let exit = gapp.run_with_args::<&str>(&[]);
    if exit != glib::ExitCode::SUCCESS {
        error!("scry exited non-success");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn on_activate(gapp: &Application, app: Arc<AppContext>) {
    style::load();

    // One deduped settings opener shared by the tray menu and overlay bar.
    let open_settings = make_settings_opener(gapp.clone(), app.clone());

    // The controller installs all overlay subscriptions and signal handlers.
    // Its signal/future closures keep it alive after activation returns.
    let _controller = overlay::OverlayController::new(gapp, app.clone(), open_settings.clone());

    install_tray_watcher(gapp.clone(), app, open_settings);
}

/// Open-or-re-present the settings window. Opening when one already exists
/// just presents it again instead of stacking duplicates.
fn make_settings_opener(gapp: Application, app: Arc<AppContext>) -> OpenSettingsFn {
    let settings_window: Rc<RefCell<Option<ApplicationWindow>>> = Rc::new(RefCell::new(None));

    Rc::new(move || {
        let mut slot = settings_window.borrow_mut();
        if let Some(existing) = slot.as_ref() {
            existing.present();
            return;
        }

        let win = settings::open(&gapp, app.clone());
        let slot_for_close = settings_window.clone();
        win.connect_close_request(move |_| {
            slot_for_close.borrow_mut().take();
            glib::Propagation::Proceed
        });
        *slot = Some(win);
    })
}

/// Tray click -> open the settings window or quit the app.
fn install_tray_watcher(gapp: Application, app: Arc<AppContext>, open_settings: OpenSettingsFn) {
    let mut rx = app.tray_events.subscribe();

    glib::spawn_future_local(async move {
        loop {
            match rx.recv().await {
                Ok(TrayEvent::OpenSettings) => open_settings(),
                Ok(TrayEvent::Quit) => {
                    gapp.quit();
                    break;
                },
                Err(RecvError::Lagged(n)) => warn!("tray: lagged by {n} events"),
                Err(RecvError::Closed) => break,
            }
        }
    });
}
