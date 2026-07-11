use std::sync::Arc;

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Button, Orientation, prelude::*};

mod picker;
mod search;
mod status;

use scry_core::AppContext;

use crate::widgets::overlay::{
    OVERLAY_WIDTH_PX,
    launcher::{picker::ModelPicker, search::Search, status::Status},
    model::{LauncherMsg, Mode, Msg, SessionMsg},
};

pub(super) const CSS: &str = include_str!("style.css");

pub(super) struct LauncherView {
    view: GtkBox,
    search: Search,
    models_status: Status,
    plugins_status: Status,
    model_picker: ModelPicker,
}

impl LauncherView {
    pub(super) fn new(
        app_context: Arc<AppContext>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Start)
            .valign(Align::Start)
            .spacing(6)
            .width_request(OVERLAY_WIDTH_PX)
            .css_classes(["scry-surface", "scry-card"])
            .build();

        let search = Search::new(dispatcher.clone());
        view.append(&search.entry);

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(11)
            .build();

        let models_status = Status::models(app_context.clone());
        let plugins_status = Status::plugins(app_context.clone());
        actions.append(models_status.widget());
        actions.append(plugins_status.widget());
        actions.append(&GtkBox::builder().hexpand(true).build());

        let controls = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(2)
            .valign(Align::Center)
            .build();

        let model_picker = ModelPicker::new(app_context.clone());
        controls.append(model_picker.widget());

        let settings_button = icon_button("emblem-system-symbolic", "Settings");
        let settings_dispatcher = dispatcher.clone();
        settings_button.connect_clicked(move |_| {
            let _ = settings_dispatcher
                .unbounded_send(Msg::Launcher(LauncherMsg::OpenSettingsRequested));
        });

        let sessions_button = icon_button("document-open-recent-symbolic", "Sessions");
        let sessions_dispatcher = dispatcher.clone();
        sessions_button.connect_clicked(move |_| {
            let _ =
                sessions_dispatcher.unbounded_send(Msg::Session(SessionMsg::ToggleViewRequested));
        });

        controls.append(&settings_button);
        controls.append(&sessions_button);
        actions.append(&controls);

        view.append(&actions);

        Self {
            view,
            search,
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

    pub(crate) fn set_mode(&self, mode: Mode) {
        self.search.set_mode(mode);
    }

    pub(crate) fn query(&self) -> String {
        self.search.query()
    }

    pub(crate) fn focus(&self) {
        self.search.focus();
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.search.has_selection()
    }

    pub(crate) fn clear(&self) {
        self.search.clear()
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
