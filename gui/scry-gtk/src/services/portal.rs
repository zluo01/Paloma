// xdg-desktop-portal GlobalShortcuts client.
//
// Asks the portal to bind a single system-wide shortcut ("summon Scry")
// and listens for activations. On KDE Wayland the portal request is
// implemented by `xdg-desktop-portal-kde`, which on first run pops up a
// dialog asking the user to confirm (or rebind) the key combo. Once
// bound, the portal remembers the binding across sessions for this
// app id.
//
// Threading: runs forever on the tokio runtime. Activations are
// fanned out via a `broadcast` channel; subscribers (the overlay)
// receive `()` per press from the GTK main context.

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures::StreamExt;
use log::{info, warn};
use tokio::sync::broadcast;

/// Stable id for our one shortcut. The portal uses (app_id, shortcut_id)
/// as the key when persisting the user's binding, so this string must
/// not change between releases or users will be re-prompted.
const SHORTCUT_ID: &str = "summon";

/// Human-readable description shown in the portal binding dialog and in
/// the desktop's keyboard-shortcut settings (e.g. KDE's "Shortcuts" KCM).
const SHORTCUT_DESCRIPTION: &str = "Summon Scry";

/// Suggested key combination. The user can override this in the portal
/// dialog. Format follows the xdg "shortcuts" spec: modifier names
/// uppercased, joined with `+`, key name lowercase.
const DEFAULT_TRIGGER: &str = "CTRL+SPACE";

/// Run the portal client forever. Returns Err only on fatal portal
/// errors; transient zbus issues are logged and the loop continues.
/// Each accepted activation is broadcast as a `()`.
pub(crate) async fn run(activations: broadcast::Sender<()>) -> ashpd::Result<()> {
    let proxy = GlobalShortcuts::new().await?;
    info!("portal: GlobalShortcuts v{} available", proxy.version());

    let session = proxy.create_session(Default::default()).await?;

    let shortcut = NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESCRIPTION)
        .preferred_trigger(Some(DEFAULT_TRIGGER));

    info!("portal: requesting shortcut bind (a system dialog may appear)…");
    let request = proxy
        .bind_shortcuts(&session, &[shortcut], None, Default::default())
        .await?;

    let response = match request.response() {
        Ok(r) => r,
        Err(e) => {
            warn!("portal: bind rejected ({e}); no hotkey is active");
            return Ok(());
        },
    };

    for s in response.shortcuts() {
        if s.id() == SHORTCUT_ID {
            info!("hotkey bound: {}", s.trigger_description());
        }
    }

    let mut activated = proxy.receive_activated().await?;
    while let Some(activation) = activated.next().await {
        if activation.shortcut_id() == SHORTCUT_ID {
            // `send` errors only if no receivers are subscribed; that's
            // fine — we still want the portal loop alive so a future
            // subscriber (overlay rebuild after a future hot-reload)
            // can still receive future activations.
            let _ = activations.send(());
        }
    }

    Ok(())
}
