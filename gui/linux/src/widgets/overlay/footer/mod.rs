mod picker;
mod status;

use std::sync::Arc;

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, Stack, StackTransitionType, prelude::*,
};
use libadwaita::ShortcutLabel;
use paloma_core::AppContext;

use crate::widgets::{
    keymap::{self, BindingId},
    overlay::{
        CHAT_VIEW_KEY, SEARCH_VIEW_KEY, SESSION_VIEW_KEY,
        footer::{picker::ModelPicker, status::Status},
        model::{LauncherMsg, Msg, SessionMsg},
    },
};

const SEARCH_HINTS: &[BindingId] = &[BindingId::SearchSubmit, BindingId::SearchShowActions];

const CHAT_HINTS: &[BindingId] = &[BindingId::ChatScrollPage];

const SESSION_HINTS: &[BindingId] = &[BindingId::SessionOpen, BindingId::SessionDelete];

pub(crate) struct FooterView {
    view: GtkBox,
    report: Stack,
    models_status: Status,
    plugins_status: Status,
    model_picker: ModelPicker,
}

const IDLE_STATUS: &str = "idle";

impl FooterView {
    pub(super) fn new(
        app_context: Arc<AppContext>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .height_request(47)
            .css_classes(["paloma-footer"])
            .build();

        // idle shows model + plugin status
        // with content, show associate shortcut hints
        let report = Stack::builder()
            .transition_type(StackTransitionType::None)
            .valign(Align::Center)
            .hhomogeneous(false)
            .build();

        let status_view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .valign(Align::Center)
            .spacing(18)
            .build();
        let models_status = Status::models(app_context.clone());
        let plugins_status = Status::plugins(app_context.clone());
        status_view.append(models_status.widget());
        status_view.append(plugins_status.widget());

        report.add_named(&status_view, Some(IDLE_STATUS));
        for (key, hints) in [
            (SEARCH_VIEW_KEY, SEARCH_HINTS),
            (CHAT_VIEW_KEY, CHAT_HINTS),
            (SESSION_VIEW_KEY, SESSION_HINTS),
        ] {
            report.add_named(&shortcut_hints(hints), Some(key));
        }

        view.append(&report);
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
            report,
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

    pub(crate) fn show_idle(&self) {
        self.report.set_visible_child_name(IDLE_STATUS);
    }

    pub(crate) fn show_search(&self) {
        self.report.set_visible_child_name(SEARCH_VIEW_KEY);
    }

    pub(crate) fn show_chat_idle(&self) {
        self.report.set_visible_child_name(CHAT_VIEW_KEY);
    }

    pub(crate) fn show_session(&self) {
        self.report.set_visible_child_name(SESSION_VIEW_KEY);
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        // keep keyboard focus on the entry so type-to-filter keeps working
        .focus_on_click(false)
        .valign(Align::Center)
        .css_classes(["flat", "circular", "dimmed"])
        .build()
}

fn shortcut_hints(hints: &[BindingId]) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::Center)
        .spacing(12)
        .build();
    for binding_id in hints {
        let hint = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .valign(Align::Center)
            .spacing(8)
            .build();
        let binding = keymap::binding(*binding_id);
        for chord in binding.shown {
            let accel = gtk4::accelerator_name(chord.accel.0, chord.accel.1);
            let short_cut = ShortcutLabel::new(accel.as_str());
            short_cut.add_css_class("dimmed");
            hint.append(&short_cut);
        }
        let description = Label::builder()
            .label(binding.label)
            .css_classes(["dimmed"])
            .build();
        hint.append(&description);
        row.append(&hint);
    }
    row
}
