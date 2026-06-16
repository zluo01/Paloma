//! The bar's preferred-model picker: a cascading provider → model menu and a
//! companion effort menu. Committing either marks the provider preferred and
//! stores its (model, effort) through the registered callback.

use gtk4::{Align, MenuButton, Popover, PopoverMenu, gio, glib, prelude::*};
use scry_core::{Connector, ProviderId};

use crate::widgets::overlay::connectors::{is_preferred, is_running};

/// Action group installed on the buttons and the detailed action name its menu
/// items target.
const GROUP: &str = "picker";
const SELECT_ACTION: &str = "picker.select";

pub(in crate::widgets::overlay) struct ModelChoice {
    pub(in crate::widgets::overlay) provider: ProviderId,
    pub(in crate::widgets::overlay) model: String,
    pub(in crate::widgets::overlay) effort: String,
}

/// The two bar controls plus the action wiring that drives them.
#[derive(Clone)]
pub(super) struct ModelPicker {
    pub(super) model_button: MenuButton,
    pub(super) effort_button: MenuButton,
    select_action: gio::SimpleAction,
}

impl ModelPicker {
    pub(super) fn new() -> Self {
        let model_button = menu_button();
        let effort_button = menu_button();
        effort_button.add_css_class("scry-effort-dropdown");

        // One `(provider, model, effort)` action backs both menus, so the
        // handler stays stateless — each item carries the whole choice.
        let action = gio::SimpleAction::new(
            "select",
            Some(glib::VariantTy::new("(sss)").expect("valid variant type")),
        );

        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&action);
        model_button.insert_action_group(GROUP, Some(&actions));
        effort_button.insert_action_group(GROUP, Some(&actions));

        Self {
            model_button,
            effort_button,
            select_action: action,
        }
    }

    pub(super) fn connect_selected(&self, cb: impl Fn(ModelChoice) + 'static) {
        self.select_action.connect_activate(move |_, param| {
            let Some((provider, model, effort)) =
                param.and_then(|p| p.get::<(String, String, String)>())
            else {
                return;
            };
            let Some(provider) = provider_from_str(&provider) else {
                return;
            };
            cb(ModelChoice {
                provider,
                model,
                effort,
            });
        });
    }

    /// Rebuild both menus and labels from `connectors`, showing only connected
    /// providers whose runtime is healthy.
    pub(super) fn set_options(&self, connectors: &[Connector]) {
        let healthy: Vec<&Connector> = connectors.iter().filter(|c| is_running(c)).collect();

        if healthy.is_empty() {
            self.reset();
            return;
        }

        // Cascading provider → model menu.
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
            model_menu.append_submenu(Some(provider_display(c.id)), &models);
        }
        self.model_button
            .set_popover(Some(&dropdown_popover(&model_menu)));
        self.model_button.set_sensitive(true);

        // Labels and the effort menu reflect the preferred (healthy) provider.
        match healthy.iter().find(|c| is_preferred(c)) {
            Some(current) => {
                let conn = current.connection.as_ref().expect("preferred is Some");
                let model = conn.status.model.iter().find(|m| m.id == conn.prefer_model);

                let effort_menu = gio::Menu::new();
                if let Some(model) = model {
                    for effort in &model.supported_reasoning_efforts {
                        effort_menu
                            .append_item(&select_item(effort, current.id, &model.id, effort));
                    }
                }
                self.effort_button
                    .set_popover(Some(&dropdown_popover(&effort_menu)));
                self.effort_button.set_sensitive(true);

                let label = model.map_or(conn.prefer_model.as_str(), |m| m.name.as_str());
                self.model_button.set_label(label);
                self.effort_button.set_label(&conn.prefer_effort);
            },
            None => {
                // Healthy providers exist, but none is preferred yet.
                self.effort_button.set_popover(Popover::NONE);
                self.effort_button.set_sensitive(false);
                self.model_button.set_label("Select model");
                self.effort_button.set_label("");
            },
        }
    }

    /// No usable providers: park both buttons in a disabled placeholder state.
    fn reset(&self) {
        self.model_button.set_popover(Popover::NONE);
        self.effort_button.set_popover(Popover::NONE);
        self.model_button.set_label("No model");
        self.effort_button.set_label("");
        self.model_button.set_sensitive(false);
        self.effort_button.set_sensitive(false);
    }
}

fn menu_button() -> MenuButton {
    let button = MenuButton::builder().valign(Align::Center).build();
    button.add_css_class("flat");
    button.add_css_class("scry-model-dropdown");
    button
}

/// A menu popover that drops straight down with no pointer arrow, so the button
/// reads as a dropdown rather than a floating popup.
fn dropdown_popover(menu: &gio::Menu) -> PopoverMenu {
    let popover = PopoverMenu::from_model(Some(menu));
    popover.set_has_arrow(false);
    popover
}

/// A menu item whose activation carries the full `(provider, model, effort)`.
fn select_item(label: &str, provider: ProviderId, model: &str, effort: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    let target = (provider.as_str(), model, effort).to_variant();
    item.set_action_and_target_value(Some(SELECT_ACTION), Some(&target));
    item
}

fn provider_display(id: ProviderId) -> &'static str {
    match id {
        ProviderId::Codex => "Codex",
        ProviderId::ClaudeCode => "Claude Code",
        ProviderId::OpenAI => "OpenAI",
        ProviderId::Anthropic => "Anthropic",
    }
}

fn provider_from_str(s: &str) -> Option<ProviderId> {
    [
        ProviderId::Codex,
        ProviderId::ClaudeCode,
        ProviderId::OpenAI,
        ProviderId::Anthropic,
    ]
    .into_iter()
    .find(|p| p.as_str() == s)
}
