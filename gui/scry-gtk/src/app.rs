//! Application bootstrap for the single-process GTK frontend.
//!
//! GTK stays on the main thread. Backend work and desktop integration services
//! run on the shared tokio runtime and report back through broadcast channels.

use std::{
    cell::{Cell, OnceCell, RefCell},
    process::ExitCode,
    rc::Rc,
    sync::Arc,
};

use gtk4::{gio, glib, prelude::*};
use libadwaita::{Application, ApplicationWindow};
use log::{error, warn};
use scry_core::AppContext;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{
    runtime::tokio_runtime,
    services::{Shortcut, TrayEvent, run_tray},
    style,
    widgets::{overlay, settings},
};

const APP_ID: &str = "dev.scry.Scry";

/// Small buffer for low-volume desktop events drained by GTK subscribers.
const UI_CHANNEL_CAPACITY: usize = 8;

/// Install the process logger if one is not already installed.
fn init_logging() {
    // Default to info, but cap zbus (D-Bus, via the portal and tray) at warn —
    // it's very chatty at info. Overridable with RUST_LOG.
    let env = env_logger::Env::default().default_filter_or("info,zbus=warn,rmcp=warn");
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_millis()
        .try_init();
}

/// Startup-built state retained for the process lifetime; the controller owns
/// the hidden overlay windows.
struct UiState {
    controller: overlay::OverlayController,
    /// `false` until the first `activate`; later activations present the overlay.
    activated_once: Cell<bool>,
}

/// Build the application, wire its lifecycle, and enter GTK.
///
/// Single-instance: a second launch re-activates the primary over D-Bus and
/// exits, so process-once setup lives in `startup` (fires once, on the primary)
/// and `activate` only presents the existing overlay.
pub(crate) fn run() -> ExitCode {
    init_logging();

    let gapp = Application::builder().application_id(APP_ID).build();

    // `startup` fills `ui` once; `activate` reads it. `startup_failed` carries a
    // fatal init failure out to the exit code, because `quit()` alone returns 0.
    let ui: Rc<OnceCell<UiState>> = Rc::new(OnceCell::new());
    let startup_failed = Rc::new(Cell::new(false));

    let ui_for_startup = Rc::clone(&ui);
    let failed_for_startup = Rc::clone(&startup_failed);
    gapp.connect_startup(move |gapp| on_startup(gapp, &ui_for_startup, &failed_for_startup));

    let ui_for_activate = Rc::clone(&ui);
    gapp.connect_activate(move |_| on_activate(&ui_for_activate));

    let exit = gapp.run_with_args::<&str>(&[]);
    if startup_failed.get() || exit != glib::ExitCode::SUCCESS {
        error!("scry startup failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Process-once setup for the primary instance. Background producers start last,
/// after their GTK consumers have subscribed.
fn on_startup(gapp: &Application, ui: &OnceCell<UiState>, startup_failed: &Cell<bool>) {
    // Build backend state on the shared tokio runtime used by later
    // `runtime::spawn` calls, so background tasks share one executor.
    let app_context = match tokio_runtime().block_on(AppContext::build()) {
        Ok(a) => a,
        Err(e) => {
            error!("{e}");
            startup_failed.set(true);
            gapp.quit();
            return;
        },
    };

    // UI coordination channels: shortcut activations summon the overlay; tray
    // menu events open settings or quit.
    let (hotkey, _) = broadcast::channel::<()>(UI_CHANNEL_CAPACITY);
    let (tray_events, _) = broadcast::channel::<TrayEvent>(UI_CHANNEL_CAPACITY);

    let shortcut = match tokio_runtime().block_on(Shortcut::init(hotkey.clone())) {
        Ok(s) => s,
        Err(e) => {
            error!("GlobalShortcuts unavailable: {e}");
            startup_failed.set(true);
            gapp.quit();
            return;
        },
    };

    // AdwApplication has initialized libadwaita and the default display by the
    // time `startup` handlers run, so CSS and adw widgets are safe here.
    style::load();

    // Share the `settings` action between the tray, overlay bar, and hotkey.
    install_settings_action(gapp, app_context.clone());

    // The controller installs overlay handlers and keeps the hidden windows alive.
    let controller = overlay::OverlayController::new(gapp, app_context, hotkey);

    install_tray_watcher(gapp.clone(), tray_events.clone());

    if ui
        .set(UiState {
            controller,
            activated_once: Cell::new(false),
        })
        .is_err()
    {
        error!("UI state already initialized");
        startup_failed.set(true);
        gapp.quit();
        return;
    }

    // Producers last: their consumers (controller hotkey watcher, tray watcher)
    // are subscribed now.
    tokio_runtime().spawn(shortcut.run());
    tokio_runtime().spawn(run_tray(tray_events));
}

/// Every launch re-activates the primary. The first activation keeps the app
/// resident (the overlay was built hidden in `startup`); later launches present
/// the existing overlay. Never rebuilds.
fn on_activate(ui: &OnceCell<UiState>) {
    let Some(state) = ui.get() else {
        return;
    };
    if !state.activated_once.replace(true) {
        return;
    }
    state.controller.present_overlay();
}

/// Install the `settings` action, reusing an open settings window if present.
fn install_settings_action(gapp: &Application, app_context: Arc<AppContext>) {
    let settings_window: Rc<RefCell<Option<ApplicationWindow>>> = Rc::new(RefCell::new(None));

    let action = gio::SimpleAction::new("settings", None);
    action.connect_activate(glib::clone!(
        #[strong]
        gapp,
        move |_, _| {
            let mut slot = settings_window.borrow_mut();
            if let Some(existing) = slot.as_ref() {
                existing.present();
                return;
            }

            let win = settings::open(&gapp, app_context.clone());
            let slot_for_close = settings_window.clone();
            win.connect_close_request(move |_| {
                slot_for_close.borrow_mut().take();
                glib::Propagation::Proceed
            });
            *slot = Some(win);
        }
    ));
    gapp.add_action(&action);
}

/// Bridge tray events into application actions on the GTK thread.
fn install_tray_watcher(gapp: Application, tray_events: broadcast::Sender<TrayEvent>) {
    let mut rx = tray_events.subscribe();

    glib::spawn_future_local(async move {
        loop {
            match rx.recv().await {
                Ok(TrayEvent::OpenSettings) => {
                    gapp.activate_action("settings", None);
                },
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
