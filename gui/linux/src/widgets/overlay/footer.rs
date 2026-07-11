use gtk4::{
    Align, Box as GtkBox, Label, Orientation, Stack, StackTransitionType,
    gdk::{Key, ModifierType},
    prelude::*,
};

use super::{CHAT_VIEW_KEY, SEARCH_VIEW_KEY, SESSION_VIEW_KEY};
use crate::widgets::keymap::{self, BindingId, Chord};

const SEARCH_HINTS: &[(BindingId, &str)] = &[
    (BindingId::SearchSubmit, "open"),
    (BindingId::SearchShowActions, "actions"),
    (BindingId::OpenSessions, "sessions"),
];

const CHAT_HINTS: &[(BindingId, &str)] = &[
    (BindingId::ChatSend, "send"),
    (BindingId::ChatInterrupt, "stop"),
    (BindingId::OpenSessions, "sessions"),
];

const SESSION_HINTS: &[(BindingId, &str)] = &[
    (BindingId::SessionOpen, "restore"),
    (BindingId::SessionDelete, "delete"),
];

pub(super) fn build() -> Stack {
    let stack = Stack::builder()
        .transition_type(StackTransitionType::None)
        .css_classes(["scry-footer"])
        .build();
    for (key, hints) in [
        (SEARCH_VIEW_KEY, SEARCH_HINTS),
        (CHAT_VIEW_KEY, CHAT_HINTS),
        (SESSION_VIEW_KEY, SESSION_HINTS),
    ] {
        stack.add_named(&hint_row(hints), Some(key));
    }
    stack
}

fn hint_row(hints: &[(BindingId, &str)]) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Center)
        .build();
    for (index, (id, wording)) in hints.iter().enumerate() {
        if index > 0 {
            row.append(&Label::new(Some("·")));
        }
        let item = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(5)
            .build();
        let keys = Label::new(Some(&accel_text(keymap::binding(*id).shown)));
        keys.add_css_class("scry-footer-key");
        item.append(&keys);
        item.append(&Label::new(Some(wording)));
        row.append(&item);
    }
    row
}

fn accel_text(chords: &[Chord]) -> String {
    chords.iter().map(chord_text).collect()
}

fn chord_text(chord: &Chord) -> String {
    let (key, mods) = chord.accel;
    let mut text = String::new();
    if mods.contains(ModifierType::CONTROL_MASK) {
        text.push_str("Ctrl+");
    }
    if mods.contains(ModifierType::SHIFT_MASK) {
        text.push_str("Shift+");
    }
    if mods.contains(ModifierType::ALT_MASK) {
        text.push_str("Alt+");
    }
    text.push_str(&key_glyph(key));
    text
}

fn key_glyph(key: Key) -> String {
    match key {
        Key::Up => "↑".into(),
        Key::Down => "↓".into(),
        Key::Return => "⏎".into(),
        Key::Escape => "Esc".into(),
        Key::Delete => "Del".into(),
        other => other
            .name()
            .map(|name| name.to_uppercase())
            .unwrap_or_default(),
    }
}
