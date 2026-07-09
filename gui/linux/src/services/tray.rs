//! System tray icon (StatusNotifierItem).
//!
//! `ScryTray` is moved into `ksni` during registration. `ksni` owns the
//! long-lived D-Bus service and calls this type for activations and menu
//! selections; callbacks broadcast `TrayEvent`s for GTK to consume.

use ksni::{
    Tray, TrayMethods,
    menu::{MenuItem, StandardItem},
};
use log::{info, warn};
use tokio::sync::broadcast;

/// A tray menu action, forwarded to the GTK thread over a broadcast channel.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TrayEvent {
    OpenSettings,
    Quit,
}

const TRAY_ID: &str = "dev.scry.Scry";
const TRAY_TITLE: &str = "Scry";
/// Freedesktop icon name; panels fall back to the title if the theme lacks it.
const TRAY_ICON: &str = "preferences-system";

/// State owned by the `ksni` service while the tray is registered.
struct ScryTray {
    events: broadcast::Sender<TrayEvent>,
}

impl ScryTray {
    fn send(&self, ev: TrayEvent) {
        // broadcast::send returns Err only if there are no receivers;
        // that's expected on shutdown and harmless.
        let _ = self.events.send(ev);
        if self.events.receiver_count() == 0 {
            warn!("tray: no subscribers for {ev:?}");
        }
    }
}

impl Tray for ScryTray {
    fn id(&self) -> String {
        TRAY_ID.into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::OpenSettings);
    }

    fn title(&self) -> String {
        TRAY_TITLE.into()
    }

    fn icon_name(&self) -> String {
        TRAY_ICON.into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Settings".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send(TrayEvent::OpenSettings);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Register the tray icon with the StatusNotifierItem host.
///
/// `ksni::TrayMethods::spawn` starts the background D-Bus service and returns
/// once registration is complete. This wrapper currently drops the returned
/// handle, so the service lives for the process lifetime and cannot be updated
/// or shut down explicitly.
pub(crate) async fn run(events: broadcast::Sender<TrayEvent>) {
    let tray = ScryTray { events };
    match tray.spawn().await {
        Ok(_handle) => info!("tray icon registered"),
        Err(e) => warn!("tray: spawn failed: {e}"),
    }
}
