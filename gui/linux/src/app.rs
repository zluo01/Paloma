use std::{
    cell::{Cell, RefCell},
    process::ExitCode,
    rc::Rc,
    sync::Arc,
};

use gtk4::{gio, glib, prelude::*};
use libadwaita::Application;
use log::{error, warn};
use paloma_core::AppContext;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{
    runtime::tokio_runtime,
    services::{Shortcut, TrayEvent, init_logging, run_tray},
    style,
    widgets::{overlay, settings::SettingsWindow},
};

const APP_ID: &str = "dev.paloma.Paloma";

const UI_CHANNEL_CAPACITY: usize = 8;

pub(crate) fn run() -> ExitCode {
    init_logging();

    let gapp = Application::builder().application_id(APP_ID).build();

    // `startup_failed` carries a fatal init failure out to the exit code,
    // because `quit()` alone returns 0.
    let startup_failed = Rc::new(Cell::new(false));

    let failed_for_startup = Rc::clone(&startup_failed);
    gapp.connect_startup(move |gapp| on_startup(gapp, &failed_for_startup));

    gapp.connect_activate(|_| {});

    let exit = gapp.run_with_args::<&str>(&[]);
    if startup_failed.get() || exit != glib::ExitCode::SUCCESS {
        error!("paloma startup failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn on_startup(gapp: &Application, startup_failed: &Cell<bool>) {
    let app_context = match tokio_runtime().block_on(AppContext::build(glib::user_data_dir())) {
        Ok(a) => a,
        Err(e) => {
            error!("{e}");
            startup_failed.set(true);
            gapp.quit();
            return;
        },
    };

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
    install_overlay_action(gapp, app_context, hotkey);

    install_tray_watcher(gapp.clone(), tray_events.clone());

    // Producers last: their consumers (controller hotkey watcher, tray watcher)
    // are subscribed now.
    tokio_runtime().spawn(shortcut.run());
    tokio_runtime().spawn(run_tray(tray_events));
}

fn install_overlay_action(
    gapp: &Application,
    app_context: Arc<AppContext>,
    hotkey: broadcast::Sender<()>,
) {
    let overlay = overlay::new(gapp, app_context, hotkey);

    let action = gio::SimpleAction::new("toggle-overlay", None);
    action.connect_activate(glib::clone!(
        #[strong]
        overlay,
        move |_, _| {
            let _ = &overlay;
        }
    ));
    gapp.add_action(&action);
}

/// Install the `settings` action, reusing an open settings window if present.
fn install_settings_action(gapp: &Application, app_context: Arc<AppContext>) {
    let slot: Rc<RefCell<Option<SettingsWindow>>> = Rc::new(RefCell::new(None));

    let action = gio::SimpleAction::new("settings", None);
    action.connect_activate(glib::clone!(
        #[strong]
        gapp,
        move |_, _| {
            slot.borrow_mut()
                .get_or_insert_with(|| SettingsWindow::new(&gapp, app_context.clone()))
                .present();
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
