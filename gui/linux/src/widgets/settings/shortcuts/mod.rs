//! Read-only reference of the overlay's keyboard shortcuts.
//!
//! Rows render from the shared [`keymap`](crate::widgets::keymap). The global
//! summon shortcut is bound through the desktop portal and configured in system
//! settings, so it is omitted here.

use gtk4::{Align, Box as GtkBox, Orientation, prelude::*};
use libadwaita::{ActionRow, PreferencesGroup, PreferencesPage, ShortcutLabel, prelude::*};

use crate::widgets::keymap::{self, Chord, Group};

pub(super) fn build() -> PreferencesPage {
    let page = PreferencesPage::new();
    for (group, title) in [
        (Group::Search, "Search"),
        (Group::Chat, "Chat"),
        (Group::Sessions, "Sessions"),
    ] {
        page.add(&render_group(title, group));
    }
    page
}

fn render_group(title: &str, group: Group) -> PreferencesGroup {
    let pref_group = PreferencesGroup::builder().title(title).build();
    for binding in keymap::group_bindings(group) {
        pref_group.add(&shortcut_row(binding.label, binding.shown));
    }
    pref_group
}

fn shortcut_row(action: &str, chords: &[Chord]) -> ActionRow {
    let row = ActionRow::builder().title(action).build();

    let caps = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    for chord in chords {
        // Only shown chords render (aliases stay hidden); GTK turns each
        // (key, mods) into its accelerator string.
        let accel = gtk4::accelerator_name(chord.accel.0, chord.accel.1);
        caps.append(&ShortcutLabel::new(accel.as_str()));
    }
    row.add_suffix(&caps);
    row
}
