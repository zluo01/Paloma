// Services tab — one expander row per supported LLM provider, with
// connect / disconnect controls and nested model-preference rows.

mod connect_modal;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use adw::{prelude::*, AlertDialog, ComboRow, ExpanderRow, ResponseAppearance};
use gtk4::{
    gdk, glib, Align, Box as GtkBox, Button, Image, ListBox, Orientation, StringList, Widget,
    Window,
};
use libadwaita as adw;
use scry_controller::ConnectorConnection;
use scry_core::AppContext;
use scry_provider::ProviderId;

use super::{section, update_placeholder, Section};
use crate::runtime;

/// The settings styling the stock theme can't express: the green
/// "Connected" subtitle and the connect modal's OTP display.
pub(crate) const CSS: &str = include_str!("style.css");

/// Static description of one provider row.
struct Provider {
    id: ProviderId,
    name: &'static str,
    /// Brand logo, bundled as SVG.
    logo: &'static [u8],
}

const PROVIDERS: &[Provider] = &[Provider {
    id: ProviderId::Codex,
    name: "Codex",
    logo: include_bytes!("assets/openai.svg"),
}];

/// The widgets of one provider row that its handlers repaint, plus the
/// context they need. Cheap to clone into signal closures.
#[derive(Clone)]
struct Card {
    provider: &'static Provider,
    row: ExpanderRow,
    /// Suffix on the collapsed row: Connect button or disconnect trash.
    actions: GtkBox,
    /// Rows currently nested in the expander, so a repaint can remove them.
    rows: Rc<RefCell<Vec<Widget>>>,
    connected: Section,
    available: Section,
    app: Arc<AppContext>,
    window: Window,
}

/// Build the Services tab; `window` parents the modals this tab opens.
pub fn build(app: Arc<AppContext>, window: Window) -> Widget {
    let connected = section("Connected", "No services connected.");
    let available = section("Available", "All supported services are connected.");

    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .valign(Align::Start)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    page.append(&connected.root);
    page.append(&available.root);

    for provider in PROVIDERS {
        let card = Card {
            provider,
            row: ExpanderRow::builder().title(provider.name).build(),
            actions: GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(12)
                .valign(Align::Center)
                .build(),
            rows: Rc::new(RefCell::new(Vec::new())),
            connected: connected.clone(),
            available: available.clone(),
            app: app.clone(),
            window: window.clone(),
        };
        card.row.add_prefix(&logo(provider));
        card.row.add_suffix(&card.actions);
        render(&card, None);
        refresh(card);
    }

    page.upcast()
}

/// Brand logo at avatar size; falls back to a generic icon if the SVG
/// can't be decoded (e.g. no SVG pixbuf loader on the system).
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

/// Move the row into the section matching its state. The row's parent
/// list is the source of truth for where it currently lives.
fn place(card: &Card, is_connected: bool) {
    let target = if is_connected {
        &card.connected
    } else {
        &card.available
    };

    if card.row.parent().as_ref() != Some(target.list.upcast_ref()) {
        if let Some(current) = card.row.parent().and_downcast::<ListBox>() {
            current.remove(&card.row);
        }
        target.list.append(&card.row);
    }

    update_placeholder(&card.connected);
    update_placeholder(&card.available);
}

/// Re-fetch the provider's connection state and repaint its row.
fn refresh(card: Card) {
    glib::MainContext::default().spawn_local(async move {
        let app = card.app.clone();
        match runtime::spawn(async move { app.connect.available_connectors().await }).await {
            Ok(connectors) => {
                let conn = connectors
                    .into_iter()
                    .find(|c| c.id == card.provider.id)
                    .and_then(|c| c.connection);
                render(&card, conn);
            },
            Err(e) => log::warn!("available_connectors failed: {e}"),
        }
    });
}

/// Repaint the row for `conn` (`None` = disconnected): move it into the
/// matching section, rebuild its suffix controls and nested rows.
fn render(card: &Card, conn: Option<ConnectorConnection>) {
    place(card, conn.is_some());

    while let Some(child) = card.actions.first_child() {
        card.actions.remove(&child);
    }
    for row in card.rows.borrow_mut().drain(..) {
        card.row.remove(&row);
    }

    let Some(conn) = conn else {
        card.row.set_subtitle("Not connected");
        card.row.remove_css_class("scry-connected");
        card.row.set_expanded(false);
        card.row.set_enable_expansion(false);
        card.actions.append(&connect_button(card));
        return;
    };

    card.row.set_subtitle("Connected");
    card.row.add_css_class("scry-connected");
    card.row.set_enable_expansion(true);
    // Collapsed by default; the user opens it when they want the pickers.
    card.row.set_expanded(false);
    add_picker_rows(card, &conn);
    append_disconnect(card);
}

fn connect_button(card: &Card) -> Button {
    let button = Button::builder()
        .label("Connect")
        .valign(Align::Center)
        .css_classes(["suggested-action"])
        .build();

    let card = card.clone();
    button.connect_clicked(move |_| {
        let on_connected: Rc<dyn Fn()> = {
            let card = card.clone();
            Rc::new(move || refresh(card.clone()))
        };
        connect_modal::open(
            &card.window,
            card.app.clone(),
            card.provider.id,
            card.provider.name,
            on_connected,
        );
    });
    button
}

/// Nest a row in the expander and remember it for the next repaint.
fn add_row(card: &Card, row: impl IsA<Widget>) {
    let row = row.upcast();
    card.row.add_row(&row);
    card.rows.borrow_mut().push(row);
}

/// Model and reasoning-effort rows. Picking a model repopulates the
/// effort list and saves that model's default effort; picking an effort
/// saves it for the current model.
fn add_picker_rows(card: &Card, conn: &ConnectorConnection) {
    let models = match &conn.status.model {
        models if !models.is_empty() => Rc::new(models.clone()),
        // No catalogue (fetch failed): show the stored preferences, read-only.
        _ => {
            for (title, value, tooltip) in [
                (
                    "Model",
                    &conn.prefer_model,
                    "Model catalogue unavailable — check the log.",
                ),
                (
                    "Effort",
                    &conn.prefer_effort,
                    "Reasoning effort unavailable — check the log.",
                ),
            ] {
                let row = ComboRow::builder()
                    .title(title)
                    .model(&StringList::new(&[value.as_str()]))
                    .sensitive(false)
                    .tooltip_text(tooltip)
                    .build();
                add_row(card, row);
            }
            return;
        },
    };

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
        let card = card.clone();
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
            save_preferences(&card, model.id.clone(), effort.clone());
        });
    }

    {
        let card = card.clone();
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
                &card,
                model.id.clone(),
                model.default_reasoning_effort.clone(),
            );
        });
    }

    add_row(card, model_row);
    add_row(card, effort_row);
}

fn save_preferences(card: &Card, model: String, effort: String) {
    let app = card.app.clone();
    let id = card.provider.id;
    glib::MainContext::default().spawn_local(async move {
        let result =
            runtime::spawn(async move { app.connect.set_preferences(id, &model, &effort).await })
                .await;
        if let Err(e) = result {
            log::warn!("set_preferences failed: {e}");
        }
    });
}

/// Trash button guarded by a confirmation dialog; on confirm, disconnects
/// the provider and repaints the row.
fn append_disconnect(card: &Card) {
    let button = Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Disconnect")
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build();

    {
        let card = card.clone();
        button.connect_clicked(move |_| {
            let dialog = AlertDialog::builder()
                .heading(format!("Disconnect {}?", card.provider.name))
                .body("Stored credentials will be removed. You'll need to sign in again to reconnect.")
                .default_response("cancel")
                .close_response("cancel")
                .build();
            dialog.add_responses(&[("cancel", "Cancel"), ("disconnect", "Disconnect")]);
            dialog.set_response_appearance("disconnect", ResponseAppearance::Destructive);

            let window = card.window.clone();
            let card = card.clone();
            dialog.connect_response(Some("disconnect"), move |_, _| {
                let card = card.clone();
                let app = card.app.clone();
                let id = card.provider.id;
                glib::MainContext::default().spawn_local(async move {
                    match runtime::spawn(async move { app.connect.disconnect(id).await }).await {
                        Ok(()) => refresh(card),
                        Err(e) => log::warn!("disconnect failed: {e}"),
                    }
                });
            });
            dialog.present(Some(&window));
        });
    }
    card.actions.append(&button);
}
