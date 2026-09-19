mod picker;
mod status;

use std::sync::Arc;

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Button, Orientation, prelude::*};
use paloma_core::AppContext;

use crate::widgets::overlay::{
    footer::{picker::ModelPicker, status::Status},
    model::{LauncherMsg, Msg, SessionMsg},
};

pub(crate) struct FooterView {
    view: GtkBox,
    models_status: Status,
    plugins_status: Status,
    model_picker: ModelPicker,
}

impl FooterView {
    pub(super) fn new(
        app_context: Arc<AppContext>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .height_request(47)
            .spacing(18)
            .css_classes(["paloma-footer"])
            .build();

        let models_status = Status::models(app_context.clone());
        let plugins_status = Status::plugins(app_context.clone());
        view.append(models_status.widget());
        view.append(plugins_status.widget());
        view.append(&GtkBox::builder().hexpand(true).build());

        let controls = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .valign(Align::Center)
            .spacing(2)
            .build();

        let model_picker = ModelPicker::new(app_context);
        controls.append(model_picker.widget());

        let settings_button = icon_button("emblem-system-symbolic", "Settings");
        let settings_dispatcher = dispatcher.clone();
        settings_button.connect_clicked(move |_| {
            let _ = settings_dispatcher
                .unbounded_send(Msg::Launcher(LauncherMsg::OpenSettingsRequested));
        });

        let sessions_button = icon_button("document-open-recent-symbolic", "Sessions");
        let sessions_dispatcher = dispatcher;
        sessions_button.connect_clicked(move |_| {
            let _ =
                sessions_dispatcher.unbounded_send(Msg::Session(SessionMsg::ToggleViewRequested));
        });

        controls.append(&settings_button);
        controls.append(&sessions_button);
        view.append(&controls);

        Self {
            view,
            models_status,
            plugins_status,
            model_picker,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn refresh(&self) {
        self.models_status.refresh();
        self.plugins_status.refresh();
        self.model_picker.refresh();
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        // keep keyboard focus on the entry so type-to-filter keeps working
        .focus_on_click(false)
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build()
}

/*
const SEARCH_HINTS: &[(BindingId, &str)] = &[
    (BindingId::SearchSubmit, "open"),
    (BindingId::SearchShowActions, "actions"),
    (BindingId::OpenSessions, "sessions"),
];

const CHAT_HINTS: &[(BindingId, &str)] = &[
    (BindingId::ChatSend, "send"),
    (BindingId::ChatInterrupt, "stop"),
    (BindingId::ChatScrollPage, "scroll"),
    (BindingId::ChatScrollEdge, "top/bottom"),
    (BindingId::OpenSessions, "sessions"),
];

const SESSION_HINTS: &[(BindingId, &str)] = &[
    (BindingId::SessionOpen, "restore"),
    (BindingId::SessionDelete, "delete"),
];

pub(super) fn build() -> Stack {
    let stack = Stack::builder()
        .transition_type(StackTransitionType::None)
        .css_classes(["paloma-footer"])
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
        keys.add_css_class("paloma-footer-key");
        item.append(&keys);
        item.append(&Label::new(Some(wording)));
        row.append(&item);
    }
    row
}

fn accel_text(chords: &[Chord]) -> String {
    chords.iter().map(chord_text).collect::<Vec<_>>().join("/")
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
        Key::Page_Up => "PgUp".into(),
        Key::Page_Down => "PgDn".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        other => other
            .name()
            .map(|name| name.to_uppercase())
            .unwrap_or_default(),
    }
}

 */
