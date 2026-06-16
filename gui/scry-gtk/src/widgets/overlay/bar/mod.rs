//! Two-row search bar card: the prompt entry on top, a status strip
//! below — labeled model/plugin health indicators on the left; model
//! selector and the settings / sessions buttons on the right. Each dot
//! aggregates a collection's health (connected models, MCP plugins).

use gtk4::{Box as GtkBox, glib, prelude::*, subclass::prelude::*};
use scry_core::{Connector, HealthLevel};

mod model_picker;
pub(super) use model_picker::ModelChoice;

/// Search bar styling: card layout, flattened entry, status indicators.
pub(super) const CSS: &str = include_str!("style.css");

mod imp {
    use std::cell::OnceCell;

    use gtk4::{
        Box as GtkBox, Button, CompositeTemplate, SearchEntry, glib, prelude::*,
        subclass::prelude::*,
    };

    use super::model_picker::ModelPicker;

    #[derive(CompositeTemplate, Default)]
    #[template(file = "bar.ui")]
    pub struct Bar {
        #[template_child]
        pub entry: TemplateChild<SearchEntry>,
        #[template_child]
        pub models_dot: TemplateChild<GtkBox>,
        #[template_child]
        pub plugins_dot: TemplateChild<GtkBox>,
        #[template_child]
        pub controls: TemplateChild<GtkBox>,
        #[template_child]
        pub settings_button: TemplateChild<Button>,
        #[template_child]
        pub sessions_button: TemplateChild<Button>,
        pub(super) picker: OnceCell<ModelPicker>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Bar {
        const NAME: &'static str = "ScryBar";
        type Type = super::Bar;
        type ParentType = GtkBox;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Bar {
        fn constructed(&self) {
            self.parent_constructed();

            // The preferred-model picker builds its own menu buttons at
            // runtime; slot them ahead of the settings/sessions buttons.
            let picker = ModelPicker::new();
            self.controls.prepend(&picker.effort_button);
            self.controls.prepend(&picker.model_button);
            let _ = self.picker.set(picker);
        }
    }

    impl WidgetImpl for Bar {}
    impl BoxImpl for Bar {}
}

glib::wrapper! {
    pub struct Bar(ObjectSubclass<imp::Bar>)
        @extends GtkBox, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl Bar {
    pub(super) fn new(width_px: i32) -> Self {
        let bar: Self = glib::Object::new();
        bar.set_width_request(width_px);
        bar
    }

    pub(super) fn focus_entry(&self) {
        self.imp().entry.grab_focus();
    }

    pub(super) fn has_selection(&self) -> bool {
        self.imp().entry.selection_bounds().is_some()
    }

    pub(super) fn input_text(&self) -> String {
        self.imp().entry.text().trim().to_string()
    }

    pub(super) fn set_input(&self, text: &str) {
        let entry = &self.imp().entry;
        entry.set_text(text);
        entry.set_position(-1);
    }

    pub(super) fn clear_input(&self) {
        self.imp().entry.set_text("");
    }

    pub(super) fn connect_search_changed(&self, cb: impl Fn(String) + 'static) {
        self.imp().entry.connect_search_changed(move |entry| {
            cb(entry.text().to_string());
        });
    }

    pub(super) fn connect_settings_clicked(&self, cb: impl Fn() + 'static) {
        self.imp().settings_button.connect_clicked(move |_| cb());
    }

    pub(super) fn connect_sessions_clicked(&self, cb: impl Fn() + 'static) {
        self.imp().sessions_button.connect_clicked(move |_| cb());
    }

    pub(super) fn connect_model_selected(&self, cb: impl Fn(ModelChoice) + 'static) {
        self.picker().connect_selected(cb);
    }

    pub(super) fn set_model_options(&self, connectors: &[Connector]) {
        self.picker().set_options(connectors);
    }

    /// Repaint both status dots from the controllers' aggregate health.
    pub(super) fn set_health(&self, models: HealthLevel, plugins: HealthLevel) {
        set_health_dot(&self.imp().models_dot, models);
        set_health_dot(&self.imp().plugins_dot, plugins);
    }

    fn picker(&self) -> &model_picker::ModelPicker {
        self.imp().picker.get().expect("picker set in constructed")
    }
}

/// CSS class for a status dot at the given health level.
fn health_level_css_class(level: &HealthLevel) -> &'static str {
    match level {
        HealthLevel::Inactive => "scry-status-inactive",
        HealthLevel::Healthy => "scry-status-healthy",
        HealthLevel::Degraded => "scry-status-degraded",
        HealthLevel::Down => "scry-status-down",
    }
}

/// Swap a dot's health class to reflect `health`.
fn set_health_dot(dot: &GtkBox, health: HealthLevel) {
    for state in [
        HealthLevel::Inactive,
        HealthLevel::Healthy,
        HealthLevel::Degraded,
        HealthLevel::Down,
    ] {
        dot.remove_css_class(health_level_css_class(&state));
    }
    dot.add_css_class(health_level_css_class(&health));
}
