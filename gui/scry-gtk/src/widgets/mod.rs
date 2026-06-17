//! GTK widget layer and small shared widget helpers.

use gtk4::prelude::*;

pub(in crate::widgets) mod keymap;
pub(crate) mod overlay;
pub(crate) mod settings;

/// Remove every child of a box.
pub(crate) fn clear_children(parent: &gtk4::Box) {
    while let Some(child) = parent.first_child() {
        parent.remove(&child);
    }
}
