use std::sync::Arc;

use gtk4::{Align, MenuButton, PopoverMenu, PopoverMenuFlags, gio, glib, prelude::*};
use log::error;
use scry_core::{AppContext, Connector, HealthStatus, ProviderId};

use crate::runtime;

const GROUP: &str = "picker";
const SELECT_ACTION: &str = "picker.select";
const DISABLED_ACTION: &str = "picker.disabled";

#[derive(Clone, Copy)]
struct Selection<'a> {
    provider: ProviderId,
    model_id: &'a str,
    model_name: &'a str,
    effort: &'a str,
}

#[derive(Clone)]
pub(super) struct ModelPicker {
    picker_button: MenuButton,
    picker_popover: PopoverMenu,
    select_action: gio::SimpleAction,
    app_context: Arc<AppContext>,
}

impl ModelPicker {
    pub(super) fn new(app_context: Arc<AppContext>) -> Self {
        let picker_button = menu_button();
        let picker_popover = dropdown_popover();
        picker_button.set_popover(Some(&picker_popover));

        // every item carries the full selected provider/model/effort triple; the
        // action's state is that same triple, so gtk checks the matching row.
        let action = gio::SimpleAction::new_stateful(
            "select",
            Some(glib::VariantTy::new("(sss)").expect("valid variant type")),
            &no_selection(),
        );

        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&action);
        // a menu item is insensitive only when its action is disabled; this no-op
        // action is what greys a row out.
        let disabled = gio::SimpleAction::new("disabled", None);
        disabled.set_enabled(false);
        actions.add_action(&disabled);
        picker_button.insert_action_group(GROUP, Some(&actions));

        let picker = Self {
            picker_button,
            picker_popover,
            select_action: action,
            app_context,
        };
        picker.reset();
        picker.connect_selected();
        picker
    }

    pub(super) fn widget(&self) -> &MenuButton {
        &self.picker_button
    }

    pub(super) fn refresh(&self) {
        refresh(self.clone());
    }

    fn connect_selected(&self) {
        let picker = self.clone();

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

            let app = picker.app_context.clone();
            let picker = picker.clone();
            runtime::spawn_with(
                async move {
                    app.set_model_preference(provider_id, &model, &effort, true)
                        .await
                },
                move |result| match result {
                    Ok(()) => refresh(picker),
                    Err(err) => error!("set preferred model: {err}"),
                },
            );
        });
    }

    fn set_options(&self, connectors: &[Connector]) {
        let selection = current_selection(connectors);
        let picker_menu = gio::Menu::new();
        let mut has_selectable_provider = false;

        for connector in connectors {
            let Some(conn) = connector.connection.as_ref() else {
                continue;
            };
            if conn.status.status != HealthStatus::Running {
                picker_menu.append_item(&disabled_item(&connector.id.to_string()));
                continue;
            }

            let current = selection.filter(|s| s.provider == connector.id);

            let models = gio::Menu::new();
            for model in &conn.status.models {
                if model.supported_reasoning_efforts.is_empty() {
                    continue;
                }

                let efforts = gio::Menu::new();
                for effort in &model.supported_reasoning_efforts {
                    efforts.append_item(&effort_item(connector.id, &model.id, effort));
                }

                let is_current_model = current.is_some_and(|s| s.model_id == model.id);
                models.append_item(&submenu_item(
                    &checked_label(&model.name, is_current_model),
                    &efforts,
                ));
            }

            let provider_label = checked_label(&connector.id.to_string(), current.is_some());
            if models.n_items() == 0 {
                picker_menu.append_item(&disabled_item(&provider_label));
            } else {
                has_selectable_provider = true;
                picker_menu.append_item(&submenu_item(&provider_label, &models));
            }
        }

        // disabled rows are context, not choices: with nothing to pick the button is dead.
        if !has_selectable_provider {
            self.reset();
            return;
        }

        self.picker_popover.set_menu_model(Some(&picker_menu));
        self.picker_button.set_sensitive(true);

        let Some(selection) = selection else {
            self.select_action.set_state(&no_selection());
            self.picker_button.set_label("Select model");
            return;
        };

        self.select_action.set_state(
            &(
                selection.provider.to_string(),
                selection.model_id,
                selection.effort,
            )
                .to_variant(),
        );
        self.picker_button
            .set_label(&format!("{} · {}", selection.model_name, selection.effort));
    }

    fn reset(&self) {
        self.picker_popover.set_menu_model(gio::MenuModel::NONE);
        self.picker_button.set_label("No model");
        self.picker_button.set_sensitive(false);
        self.select_action.set_state(&no_selection());
    }
}

/// The stored preference, but only while it still resolves to a live row. An unhealthy
/// provider, a model the catalogue dropped, or an effort it no longer offers is not a
/// selection: nothing is checked and the user picks again.
fn current_selection(connectors: &[Connector]) -> Option<Selection<'_>> {
    connectors.iter().find_map(|connector| {
        let conn = connector.connection.as_ref()?;
        if !conn.preferred || conn.status.status != HealthStatus::Running {
            return None;
        }

        let model = conn
            .status
            .models
            .iter()
            .find(|m| m.id == conn.prefer_model)?;

        model
            .supported_reasoning_efforts
            .contains(&conn.prefer_effort)
            .then_some(Selection {
                provider: connector.id,
                model_id: &model.id,
                model_name: &model.name,
                effort: &conn.prefer_effort,
            })
    })
}

fn refresh(picker: ModelPicker) {
    let app_context = picker.app_context.clone();
    runtime::spawn_with(
        async move { app_context.available_connectors().await },
        move |result| match result {
            Ok(connectors) => picker.set_options(&connectors),
            Err(err) => error!("model picker refresh: {err}"),
        },
    );
}

fn menu_button() -> MenuButton {
    MenuButton::builder()
        .valign(Align::Center)
        // keep keyboard focus on the entry so type-to-filter keeps working
        .focus_on_click(false)
        .css_classes(["flat", "scry-model-dropdown"])
        .build()
}

fn dropdown_popover() -> PopoverMenu {
    let popover = PopoverMenu::from_model_full(&gio::Menu::new(), PopoverMenuFlags::NESTED);
    popover.set_has_arrow(false);
    popover
}

fn effort_item(provider: ProviderId, model: &str, effort: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(effort), None);
    let target = (provider.to_string(), model, effort).to_variant();
    item.set_action_and_target_value(Some(SELECT_ACTION), Some(&target));
    item
}

fn submenu_item(label: &str, submenu: &gio::Menu) -> gio::MenuItem {
    gio::MenuItem::new_submenu(Some(label), submenu)
}

fn disabled_item(label: &str) -> gio::MenuItem {
    gio::MenuItem::new(Some(label), Some(DISABLED_ACTION))
}

// submenu and disabled rows carry no action, so gtk cannot check them for us.
fn checked_label(label: &str, checked: bool) -> String {
    if checked {
        format!("✓ {label}")
    } else {
        label.to_string()
    }
}

fn no_selection() -> glib::Variant {
    ("", "", "").to_variant()
}
