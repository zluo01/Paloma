//! Global summon shortcut via xdg-desktop-portal GlobalShortcuts.
//!
//! The shortcut service owns the portal proxy/session and broadcasts accepted
//! activations to GTK subscribers. It does not touch GTK directly.

use ashpd::desktop::{
    Session,
    global_shortcuts::{GlobalShortcuts, NewShortcut},
};
use futures::StreamExt;
use log::{info, warn};
use tokio::sync::broadcast;

/// Stable shortcut id. The portal persists bindings by `(app_id, shortcut_id)`,
/// so changing this prompts users to bind the shortcut again.
const SHORTCUT_ID: &str = "summon";
/// Label shown in the portal bind dialog and desktop shortcut settings.
const SHORTCUT_DESCRIPTION: &str = "Summon Scry";
/// Suggested key combination; users may override it in the portal dialog.
const DEFAULT_TRIGGER: &str = "CTRL+SPACE";

/// Live GlobalShortcuts session.
///
/// [`init`](Self::init) performs fatal setup. [`run`](Self::run) performs the
/// user-facing bind request and then forwards activations.
pub(crate) struct Shortcut {
    proxy: GlobalShortcuts,
    /// Kept alive for the whole listener lifetime; dropping it ends the portal
    /// session and its bindings.
    session: Session<GlobalShortcuts>,
    activations: broadcast::Sender<()>,
}

impl Shortcut {
    /// Create the portal proxy and session.
    ///
    /// Errors here mean GlobalShortcuts is unavailable, so the caller should
    /// fail startup. The user-facing bind request is deferred to
    /// [`run`](Self::run).
    pub(crate) async fn init(activations: broadcast::Sender<()>) -> ashpd::Result<Self> {
        let proxy = GlobalShortcuts::new().await?;
        let session = proxy.create_session(Default::default()).await?;
        Ok(Self {
            proxy,
            session,
            activations,
        })
    }

    /// Request the shortcut binding and forward activations until the portal
    /// stream ends.
    ///
    /// Binding rejection is non-fatal: the app keeps running, but no hotkey is
    /// active for this session.
    pub(crate) async fn run(self) {
        match self.bind().await {
            Ok(desc) => info!("shortcut: bound {desc}"),
            Err(e) => warn!("shortcut: not bound ({e}); no hotkey is active"),
        }

        let mut activated = match self.proxy.receive_activated().await {
            Ok(stream) => stream,
            Err(e) => {
                warn!("shortcut: cannot receive activations ({e})");
                return;
            },
        };
        while let Some(activation) = activated.next().await {
            if activation.shortcut_id() == SHORTCUT_ID {
                let _ = self.activations.send(());
            }
        }
    }

    /// Request the binding and return the desktop's trigger description.
    async fn bind(&self) -> ashpd::Result<String> {
        let shortcut = NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESCRIPTION)
            .preferred_trigger(Some(DEFAULT_TRIGGER));
        let request = self
            .proxy
            .bind_shortcuts(&self.session, &[shortcut], None, Default::default())
            .await?;
        let response = request.response()?;

        Ok(response
            .shortcuts()
            .iter()
            .find(|s| s.id() == SHORTCUT_ID)
            .map(|s| s.trigger_description().to_string())
            .unwrap_or_default())
    }
}
