use std::sync::Arc;

use gtk4::{Align, MenuButton, Popover, PopoverMenu, gio, glib, prelude::*};
use log::error;
use scry_core::{AppContext, Connector, HealthStatus, ProviderId};

use crate::runtime;

const GROUP: &str = "picker";
const SELECT_ACTION: &str = "picker.select";

#[derive(Clone)]
pub(super) struct ModelPicker {
    model_button: MenuButton,
    effort_button: MenuButton,
    select_action: gio::SimpleAction,
    app_context: Arc<AppContext>,
}

impl ModelPicker {
    pub(super) fn new(app_context: Arc<AppContext>) -> Self {
        let model_button = menu_button();
        let effort_button = menu_button();

        // every item carries the full selected provider/model/effort triple.
        let action = gio::SimpleAction::new(
            "select",
            Some(glib::VariantTy::new("(sss)").expect("valid variant type")),
        );

        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&action);
        model_button.insert_action_group(GROUP, Some(&actions));
        effort_button.insert_action_group(GROUP, Some(&actions));

        let picker = Self {
            model_button,
            effort_button,
            select_action: action,
            app_context,
        };
        picker.connect_selected();
        picker.refresh();
        picker
    }

    pub(super) fn model_dropdown(&self) -> &MenuButton {
        &self.model_button
    }

    pub(super) fn effort_dropdown(&self) -> &MenuButton {
        &self.effort_button
    }

    pub(super) fn refresh(&self) {
        refresh(
            self.model_button.clone(),
            self.effort_button.clone(),
            self.app_context.clone(),
        );
    }

    fn connect_selected(&self) {
        let model_button = self.model_button.clone();
        let effort_button = self.effort_button.clone();
        let app_context = self.app_context.clone();

        self.select_action.connect_activate(move |_, param| {
            let Some((provider, model, effort)) =
                param.and_then(|p| p.get::<(String, String, String)>())
            else {
                return;
            };
            let Ok(provider_id) = provider.parse::<ProviderId>() else {
                error!("unknown provider string {}, this indicate a bug.", provider);
                return;
            };

            let app = app_context.clone();
            let refresh_app = app_context.clone();
            let model_button = model_button.clone();
            let effort_button = effort_button.clone();
            runtime::spawn_with(
                async move { app.set_model_preference(provider_id, &model, &effort).await },
                move |result| match result {
                    Ok(()) => refresh(model_button, effort_button, refresh_app),
                    Err(err) => error!("set preferred model: {err}"),
                },
            );
        });
    }
}

fn refresh(model_button: MenuButton, effort_button: MenuButton, app_context: Arc<AppContext>) {
    runtime::spawn_with(
        async move { app_context.available_connectors().await },
        move |result| match result {
            Ok(connectors) => set_options(&model_button, &effort_button, &connectors),
            Err(err) => error!("model picker refresh: {err}"),
        },
    );
}

fn set_options(model_button: &MenuButton, effort_button: &MenuButton, connectors: &[Connector]) {
    let healthy: Vec<&Connector> = connectors.iter().filter(|c| is_running(c)).collect();

    if healthy.is_empty() {
        reset(model_button, effort_button);
        return;
    }

    let model_menu = gio::Menu::new();
    for c in &healthy {
        let conn = c.connection.as_ref().expect("filtered to Some");
        let models = gio::Menu::new();
        for model in &conn.status.model {
            models.append_item(&select_item(
                &model.name,
                c.id,
                &model.id,
                &model.default_reasoning_effort,
            ));
        }
        model_menu.append_submenu(Some(&c.id.to_string()), &models);
    }
    model_button.set_popover(Some(&dropdown_popover(&model_menu)));
    model_button.set_sensitive(true);

    match healthy.iter().find(|c| is_preferred(c)) {
        Some(current) => {
            let conn = current.connection.as_ref().expect("preferred is Some");
            let model = conn.status.model.iter().find(|m| m.id == conn.prefer_model);

            let effort_menu = gio::Menu::new();
            if let Some(model) = model {
                for effort in &model.supported_reasoning_efforts {
                    effort_menu.append_item(&select_item(effort, current.id, &model.id, effort));
                }
            }
            effort_button.set_popover(Some(&dropdown_popover(&effort_menu)));
            effort_button.set_sensitive(true);

            let label = model.map_or(conn.prefer_model.as_str(), |m| m.name.as_str());
            model_button.set_label(label);
            effort_button.set_label(&conn.prefer_effort);
        },
        None => {
            effort_button.set_popover(Popover::NONE);
            effort_button.set_sensitive(false);
            model_button.set_label("Select model");
            effort_button.set_label("");
        },
    }
}

fn reset(model_button: &MenuButton, effort_button: &MenuButton) {
    model_button.set_popover(Popover::NONE);
    effort_button.set_popover(Popover::NONE);
    model_button.set_label("No model");
    effort_button.set_label("");
    model_button.set_sensitive(false);
    effort_button.set_sensitive(false);
}

fn menu_button() -> MenuButton {
    MenuButton::builder()
        .valign(Align::Center)
        .css_classes(["flat", "scry-model-dropdown"])
        .build()
}

fn dropdown_popover(menu: &gio::Menu) -> PopoverMenu {
    let popover = PopoverMenu::from_model(Some(menu));
    popover.set_has_arrow(false);
    popover
}

fn select_item(label: &str, provider: ProviderId, model: &str, effort: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    let target = (provider.to_string(), model, effort).to_variant();
    item.set_action_and_target_value(Some(SELECT_ACTION), Some(&target));
    item
}

fn is_running(connector: &Connector) -> bool {
    connector
        .connection
        .as_ref()
        .is_some_and(|c| c.status.status == HealthStatus::Running)
}

fn is_preferred(connector: &Connector) -> bool {
    connector.connection.as_ref().is_some_and(|c| c.preferred)
}
