mod connect_modal;

use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc};

use gtk4::{Align, Box as GtkBox, Button, Image, Orientation, StringList, gdk, glib};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, ComboRow, ExpanderRow, PreferencesPage,
    ResponseAppearance, prelude::*,
};
use scry_core::{AppContext, Connector, ConnectorConnection, HealthStatus, ProviderId};

use super::{Group, unhealthy_icon};
use crate::runtime;

pub(super) const CSS: &str = include_str!("style.css");

struct Provider {
    id: ProviderId,
    name: &'static str,
    logo: &'static [u8],
}

const PROVIDERS: &[Provider] = &[Provider {
    id: ProviderId::Codex,
    name: "Codex",
    logo: include_bytes!("assets/openai.svg"),
}];

pub(super) fn build(app: Arc<AppContext>, window: ApplicationWindow) -> PreferencesPage {
    let content = ServiceContent {
        connected: Group::new("Connected"),
        available: Group::new("Available"),
        app,
        window,
    };

    let page = PreferencesPage::new();
    page.add(&content.connected.widget);
    page.add(&content.available.widget);

    content.refresh();
    page
}

#[derive(Clone)]
struct ServiceContent {
    connected: Group,
    available: Group,
    app: Arc<AppContext>,
    window: ApplicationWindow,
}

impl ServiceContent {
    fn refresh(&self) {
        let content = self.clone();
        let app = self.app.clone();
        runtime::spawn_with(
            async move { app.connect.available_connectors().await },
            move |result| match result {
                Ok(connectors) => content.render(connectors),
                Err(e) => log::warn!("available_connectors failed: {e}"),
            },
        );
    }

    fn render(&self, connectors: Vec<Connector>) {
        self.connected.clear();
        self.available.clear();

        let mut connections: HashMap<ProviderId, ConnectorConnection> = connectors
            .into_iter()
            .filter_map(|c| c.connection.map(|conn| (c.id, conn)))
            .collect();

        for provider in PROVIDERS {
            match connections.remove(&provider.id) {
                Some(conn) => self.connected.add(self.connected_row(provider, conn)),
                None => self.available.add(self.available_row(provider)),
            }
        }

        if self.connected.is_empty() {
            self.connected.add(placeholder("No services connected."));
        }
        if self.available.is_empty() {
            self.available
                .add(placeholder("All supported services are connected."));
        }
    }

    fn available_row(&self, provider: &'static Provider) -> ActionRow {
        let row = ActionRow::builder()
            .title(provider.name)
            .subtitle("Not connected")
            .build();
        row.add_prefix(&logo(provider));
        row.add_suffix(&self.connect_button(provider));
        row
    }

    fn connected_row(&self, provider: &'static Provider, conn: ConnectorConnection) -> ExpanderRow {
        let row = ExpanderRow::builder().title(provider.name).build();
        row.add_prefix(&logo(provider));

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .valign(Align::Center)
            .build();

        if conn.status.status == HealthStatus::Running {
            row.set_subtitle("Connected");
            row.add_css_class("scry-connected");
            if conn.status.model.is_empty() {
                row.set_enable_expansion(false);
            } else {
                add_picker_rows(&self.app, provider.id, &row, &conn);
            }
        } else {
            row.set_subtitle("Connection error");
            row.add_css_class("scry-error");
            row.set_enable_expansion(false);
            actions.append(&unhealthy_icon(conn.status.error.as_deref()));
        }

        actions.append(&self.disconnect_button(provider));
        row.add_suffix(&actions);
        row
    }

    fn connect_button(&self, provider: &'static Provider) -> Button {
        let button = Button::builder()
            .label("Connect")
            .valign(Align::Center)
            .css_classes(["suggested-action"])
            .build();

        let content = self.clone();
        button.connect_clicked(move |_| {
            let on_connected: Rc<dyn Fn()> = {
                let content = content.clone();
                Rc::new(move || content.refresh())
            };
            connect_modal::open(
                &content.window,
                content.app.clone(),
                provider.id,
                provider.name,
                on_connected,
            );
        });
        button
    }

    fn disconnect_button(&self, provider: &'static Provider) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Disconnect")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let content = self.clone();
        button.connect_clicked(move |_| {
            let dialog = AlertDialog::builder()
                .heading(format!("Disconnect {}?", provider.name))
                .body("Stored credentials will be removed. You'll need to sign in again to reconnect.")
                .default_response("cancel")
                .close_response("cancel")
                .build();
            dialog.add_responses(&[("cancel", "Cancel"), ("disconnect", "Disconnect")]);
            dialog.set_response_appearance("disconnect", ResponseAppearance::Destructive);

            dialog.connect_response(Some("disconnect"), {
                let content = content.clone();
                move |_, _| {
                    let content = content.clone();
                    let app = content.app.clone();
                    let id = provider.id;
                    runtime::spawn_with(
                        async move { app.connect.disconnect(id).await },
                        move |result| match result {
                            Ok(()) => content.refresh(),
                            Err(e) => log::warn!("disconnect failed: {e}"),
                        },
                    );
                }
            });
            dialog.present(Some(&content.window));
        });
        button
    }
}

fn placeholder(text: &str) -> ActionRow {
    ActionRow::builder()
        .title(text)
        .css_classes(["dim-label"])
        .build()
}

fn logo(provider: &Provider) -> Image {
    let image = match gdk::Texture::from_bytes(&glib::Bytes::from_static(provider.logo)) {
        Ok(texture) => Image::from_paintable(Some(&texture)),
        Err(e) => {
            log::warn!("failed to load {} logo: {e}", provider.name);
            Image::from_icon_name("application-x-executable-symbolic")
        },
    };
    image.set_pixel_size(40);
    image
}

/// Choosing a model repopulates the effort list with that model's default.
fn add_picker_rows(
    app: &Arc<AppContext>,
    id: ProviderId,
    row: &ExpanderRow,
    conn: &ConnectorConnection,
) {
    let models = Rc::new(conn.status.model.clone());

    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    let model_row = ComboRow::builder()
        .title("Model")
        .model(&StringList::new(&ids))
        .build();
    let model_idx = models
        .iter()
        .position(|m| m.id == conn.prefer_model)
        .unwrap_or(0);
    model_row.set_selected(model_idx as u32);

    let efforts: Vec<&str> = models[model_idx]
        .supported_reasoning_efforts
        .iter()
        .map(|s| s.as_str())
        .collect();
    let effort_row = ComboRow::builder()
        .title("Effort")
        .model(&StringList::new(&efforts))
        .build();
    if let Some(idx) = models[model_idx]
        .supported_reasoning_efforts
        .iter()
        .position(|e| e == &conn.prefer_effort)
    {
        effort_row.set_selected(idx as u32);
    }

    // Suppresses the effort handler while the model handler repopulates
    // the effort list.
    let muted = Rc::new(Cell::new(false));

    {
        let app = app.clone();
        let models = models.clone();
        let model_row = model_row.clone();
        let muted = muted.clone();
        effort_row.connect_selected_notify(move |row| {
            if muted.get() {
                return;
            }
            let Some(model) = models.get(model_row.selected() as usize) else {
                return;
            };
            let Some(effort) = model
                .supported_reasoning_efforts
                .get(row.selected() as usize)
            else {
                return;
            };
            save_preferences(&app, id, model.id.clone(), effort.clone());
        });
    }

    {
        let app = app.clone();
        let effort_row = effort_row.clone();
        let models = models.clone();
        model_row.connect_selected_notify(move |row| {
            let Some(model) = models.get(row.selected() as usize) else {
                return;
            };
            let efforts: Vec<&str> = model
                .supported_reasoning_efforts
                .iter()
                .map(|s| s.as_str())
                .collect();
            let default = model
                .supported_reasoning_efforts
                .iter()
                .position(|e| e == &model.default_reasoning_effort)
                .unwrap_or(0);

            muted.set(true);
            effort_row.set_model(Some(&StringList::new(&efforts)));
            if !efforts.is_empty() {
                effort_row.set_selected(default as u32);
            }
            muted.set(false);

            save_preferences(
                &app,
                id,
                model.id.clone(),
                model.default_reasoning_effort.clone(),
            );
        });
    }

    row.add_row(&model_row);
    row.add_row(&effort_row);
}

fn save_preferences(app: &Arc<AppContext>, id: ProviderId, model: String, effort: String) {
    let app = app.clone();
    runtime::spawn_with(
        async move { app.connect.set_preferences(id, &model, &effort).await },
        |result| {
            if let Err(e) = result {
                log::warn!("set_preferences failed: {e}");
            }
        },
    );
}
