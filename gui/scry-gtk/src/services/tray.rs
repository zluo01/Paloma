// System tray icon (StatusNotifierItem).
//
// Pure DBus via `ksni`; no GTK in this module. Activations and menu
// selections are forwarded as `TrayEvent` over a broadcast channel
// consumed by the main app on the GTK thread.
//
// Activation model (KDE convention):
//   * Left click  → `activate` → OpenSettings
//   * Right click → context menu with "Open Settings" + "Quit"

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
/// Freedesktop icon name. Themes that ship `preferences-system` (most
/// of them) will render a gear; otherwise the panel falls back to the
/// item's title text.
const TRAY_ICON: &str = "preferences-system";

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

/// Spawn the tray icon. Returns once the SNI registration completes
/// (or is logged-and-ignored if the host is unavailable).
pub(crate) async fn run(events: broadcast::Sender<TrayEvent>) {
    let tray = ScryTray { events };
    match tray.spawn().await {
        Ok(_handle) => info!("tray icon registered"),
        Err(e) => warn!("tray: spawn failed: {e}"),
    }
}
