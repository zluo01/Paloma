//! Global stylesheet loader.
//!
//! Each UI component owns its CSS as a `const CSS` next to
//! the widget code (embedded via `include_str!` so the binary is
//! self-contained); the top-level UI modules aggregate them into
//! `CSS_PARTS` slices, which this loader concatenates once at startup
//! into a single `CssProvider` on the default display. No two
//! components target the same selector, so ordering doesn't matter.

use gtk4::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION, gdk::Display};

/// Load the global stylesheet onto the default display.
///
/// Must be called after GTK is initialised (i.e. inside `connect_activate`)
/// because it needs `Display::default()`. Idempotent — calling twice just
/// re-installs the same provider.
pub(crate) fn load() {
    let combined = collect();

    let provider = CssProvider::new();
    provider.load_from_string(&combined);

    if let Some(display) = Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    } else {
        // Should never happen on a working Wayland session; log loudly
        // because if styles fail to load the overlay falls back to
        // Adwaita defaults which look wrong (focus rings, white fill).
        log::error!("no GDK display; styles not applied");
    }
}

/// Concatenate every contributing module's CSS into a single string.
fn collect() -> String {
    let parts = crate::widgets::overlay::CSS_PARTS
        .iter()
        .chain(crate::widgets::settings::CSS_PARTS.iter());

    let cap: usize = parts.clone().map(|s| s.len()).sum();
    let mut buf = String::with_capacity(cap);
    for part in parts {
        buf.push_str(part);
    }
    buf
}
