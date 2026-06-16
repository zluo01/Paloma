//! The View layer: GObject-subclassed widgets and their composite
//! templates, plus small shared widget helpers.

use gtk4::prelude::*;

pub(crate) mod overlay;
pub(crate) mod settings;

/// Remove every child of a box. GTK4 has no universal "remove all
/// children", so each container clears via its own `remove`; this covers
/// the common `gtk4::Box` case the UI rebuilds in place.
pub(crate) fn clear_children(parent: &gtk4::Box) {
    while let Some(child) = parent.first_child() {
        parent.remove(&child);
    }
}
