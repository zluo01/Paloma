//! Global stylesheet loader.
//!
//! Component CSS is embedded next to the widget code, collected through
//! `CSS_PARTS`, and installed once as a single provider.

use gtk4::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION, gdk::Display};

/// Load the global stylesheet onto the default display.
///
/// Requires an initialized GTK display; app startup calls this after the
/// default display exists. Calling twice re-installs the same provider.
pub(crate) fn load() {
    let combined = collect();

    let provider = CssProvider::new();
    provider.load_from_string(&combined);

    let display = Display::default().expect("GDK display must exist before loading GTK styles");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Build one stylesheet from all component fragments.
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
