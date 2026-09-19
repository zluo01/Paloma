use std::sync::Arc;

use gtk4::{Align, Box as GtkBox, Label, Orientation, prelude::*};
use paloma_core::{AppContext, HealthLevel};

use crate::runtime;

#[derive(Clone, Copy)]
enum StatusKind {
    Models,
    Plugins,
}

pub(super) struct Status {
    view: GtkBox,
    dot: GtkBox,
    app: Arc<AppContext>,
    kind: StatusKind,
}

impl Status {
    pub(super) fn models(app: Arc<AppContext>) -> Self {
        Self::new("Models", app, StatusKind::Models)
    }

    pub(super) fn plugins(app: Arc<AppContext>) -> Self {
        Self::new("Plugins", app, StatusKind::Plugins)
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.view
    }

    fn new(label: &str, app: Arc<AppContext>, kind: StatusKind) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .valign(Align::Center)
            .build();

        let dot = GtkBox::builder()
            .valign(Align::Center)
            .css_classes(["paloma-status-dot", "paloma-status-inactive"])
            .build();

        let label = Label::new(Some(label));
        label.add_css_class("paloma-status-label");

        view.append(&dot);
        view.append(&label);

        Self {
            view,
            dot,
            app,
            kind,
        }
    }

    pub(super) fn refresh(&self) {
        let app = self.app.clone();
        let dot = self.dot.clone();
        let kind = self.kind;

        runtime::spawn_with(
            async move {
                match kind {
                    StatusKind::Models => app.connectors_health_level().await,
                    StatusKind::Plugins => app.plugins_health_level().await,
                }
            },
            move |health| set_health_dot(&dot, health),
        );
    }
}

fn set_health_dot(dot: &GtkBox, health: HealthLevel) {
    let status_css = match health {
        HealthLevel::Inactive => "paloma-status-inactive",
        HealthLevel::Healthy => "paloma-status-healthy",
        HealthLevel::Degraded => "paloma-status-degraded",
        HealthLevel::Down => "paloma-status-down",
    };
    dot.set_css_classes(&["paloma-status-dot", status_css]);
}
