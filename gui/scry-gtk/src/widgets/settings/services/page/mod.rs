mod model;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use gtk4::{Align, Box as GtkBox, Button, Image, Orientation, StringList};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, ComboRow, ExpanderRow, PreferencesGroup,
    PreferencesPage, ResponseAppearance,
    gdk::Texture,
    glib::{Bytes, WeakRef},
    prelude::*,
};
use scry_core::{AppContext, ConnectorConnection, HealthStatus, ProviderId};

use self::model::{Command, Model, Msg};
use super::modal;
use crate::{
    helper::Clear,
    runtime,
    widgets::settings::helper::{group_is_empty, placeholder, unhealthy_icon},
};

struct Provider {
    id: ProviderId,
    logo: &'static [u8],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: ProviderId::Anthropic,
        logo: include_bytes!("assets/claude.svg"),
    },
    Provider {
        id: ProviderId::ClaudeCode,
        logo: include_bytes!("assets/claude.svg"),
    },
    Provider {
        id: ProviderId::Codex,
        logo: include_bytes!("assets/openai.svg"),
    },
    Provider {
        id: ProviderId::OpenAI,
        logo: include_bytes!("assets/openai.svg"),
    },
];

pub(crate) struct ServicesPage {
    view: PreferencesPage,
    connected_group: PreferencesGroup,
    available_group: PreferencesGroup,
    app: Arc<AppContext>,
    window: WeakRef<ApplicationWindow>,
    model: RefCell<Model>,
}

impl ServicesPage {
    pub(crate) fn new(app: Arc<AppContext>, window: &ApplicationWindow) -> Rc<Self> {
        let (view, connected_group, available_group) = build_view();

        Rc::new(Self {
            view,
            connected_group,
            available_group,
            app,
            window: window.downgrade(),
            model: RefCell::new(Model::default()),
        })
    }

    pub(crate) fn widget(&self) -> &PreferencesPage {
        &self.view
    }

    pub(crate) fn refresh(self: &Rc<Self>) {
        let app = self.app.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app.available_connectors().await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::ConnectorsFetched(result));
                }
            },
        );
    }

    fn dispatch(self: &Rc<Self>, msg: Msg) {
        let commands = self.model.borrow_mut().update(msg);
        for command in commands {
            self.run(command);
        }
    }

    fn run(self: &Rc<Self>, command: Command) {
        match command {
            Command::Render => self.render(),
            Command::FetchConnectors => self.refresh(),
            Command::ShowConnectDialog(id) => self.show_connect_dialog(id),
            Command::ShowDisconnectConfirmation(id) => self.show_disconnect_confirmation(id),
            Command::DisconnectProvider(id) => self.disconnect_provider(id),
            Command::PersistPreference { id, model, effort } => {
                self.persist_preference(id, model, effort)
            },
            Command::Warn(message) => log::warn!("{message}"),
        }
    }

    fn disconnect_provider(self: &Rc<Self>, id: ProviderId) {
        let app = self.app.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app.disconnect_connector(id).await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::DisconnectFinished(id, result));
                }
            },
        );
    }

    fn persist_preference(self: &Rc<Self>, id: ProviderId, model: String, effort: String) {
        let app = self.app.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app.set_model_preference(id, &model, &effort).await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::PreferenceSaveFinished(result));
                }
            },
        );
    }

    fn show_connect_dialog(self: &Rc<Self>, id: ProviderId) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let on_connected: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::ConnectionSucceeded);
            }
        });
        modal::open(&window, self.app.clone(), id, on_connected);
    }

    fn show_disconnect_confirmation(self: &Rc<Self>, id: ProviderId) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let dialog = AlertDialog::builder()
            .heading(format!("Disconnect {id}?"))
            .body("Stored credentials will be removed. You'll need to sign in again to reconnect.")
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("disconnect", "Disconnect")]);
        dialog.set_response_appearance("disconnect", ResponseAppearance::Destructive);

        let weak = Rc::downgrade(self);
        dialog.connect_response(Some("disconnect"), move |_, _| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::DisconnectConfirmed(id));
            }
        });
        dialog.present(Some(&window));
    }

    fn render(self: &Rc<Self>) {
        self.connected_group.clear();
        self.available_group.clear();

        let state = self.model.borrow();
        for provider in PROVIDERS {
            let connection = state
                .connectors
                .iter()
                .find(|connector| connector.id == provider.id)
                .and_then(|connector| connector.connection.as_ref());
            match connection {
                Some(conn) => self
                    .connected_group
                    .add(&self.connected_row(provider, conn)),
                None => self.available_group.add(&self.available_row(provider)),
            }
        }

        if group_is_empty(&self.connected_group) {
            self.connected_group
                .add(&placeholder("No services connected."));
        }
        if group_is_empty(&self.available_group) {
            self.available_group
                .add(&placeholder("All supported services are connected."));
        }
    }

    fn available_row(self: &Rc<Self>, provider: &'static Provider) -> ActionRow {
        let row = ActionRow::builder().title(provider.id.to_string()).build();
        row.add_prefix(&logo(provider));
        row.add_suffix(&self.connect_button(provider));
        row
    }

    fn connected_row(
        self: &Rc<Self>,
        provider: &'static Provider,
        conn: &ConnectorConnection,
    ) -> ExpanderRow {
        let row = ExpanderRow::builder()
            .title(provider.id.to_string())
            .build();
        row.add_prefix(&logo(provider));

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .valign(Align::Center)
            .build();

        if conn.status.status == HealthStatus::Running {
            row.set_subtitle("Connected");
            row.add_css_class("scry-connected");
            if conn.status.model.is_empty() {
                row.set_enable_expansion(false);
            } else {
                self.add_picker_rows(provider.id, &row, conn);
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

    fn connect_button(self: &Rc<Self>, provider: &'static Provider) -> Button {
        let button = Button::builder()
            .label("Connect")
            .valign(Align::Center)
            .css_classes(["suggested-action"])
            .build();

        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::ConnectClicked(provider.id));
            }
        });
        button
    }

    fn disconnect_button(self: &Rc<Self>, provider: &'static Provider) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Disconnect")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::DisconnectClicked(provider.id));
            }
        });
        button
    }

    fn add_picker_rows(
        self: &Rc<Self>,
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
        if !efforts.is_empty() {
            let effort_idx = models[model_idx]
                .supported_reasoning_efforts
                .iter()
                .position(|e| e == &conn.prefer_effort)
                .unwrap_or(0);
            effort_row.set_selected(effort_idx as u32);
        }

        // Rebuilding the effort list emits selection notifications; ignore those
        // so only the model handler dispatches the model's default effort.
        let suppress_effort_notify = Rc::new(Cell::new(false));

        let weak = Rc::downgrade(self);
        let models_for_effort = models.clone();
        let suppress_effort_notify_for_effort = suppress_effort_notify.clone();
        let model_row_weak = model_row.downgrade();
        effort_row.connect_selected_notify(move |row| {
            if suppress_effort_notify_for_effort.get() {
                return;
            }
            let Some(model_row) = model_row_weak.upgrade() else {
                return;
            };
            let Some(model) = models_for_effort.get(model_row.selected() as usize) else {
                return;
            };
            let Some(effort) = model
                .supported_reasoning_efforts
                .get(row.selected() as usize)
            else {
                return;
            };
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::PreferenceChanged {
                    id,
                    model: model.id.clone(),
                    effort: effort.clone(),
                });
            }
        });

        let weak = Rc::downgrade(self);
        let effort_row_weak = effort_row.downgrade();
        model_row.connect_selected_notify(move |row| {
            let Some(effort_row) = effort_row_weak.upgrade() else {
                return;
            };
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

            suppress_effort_notify.set(true);
            effort_row.set_model(Some(&StringList::new(&efforts)));
            if !efforts.is_empty() {
                effort_row.set_selected(default as u32);
            }
            suppress_effort_notify.set(false);

            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::PreferenceChanged {
                    id,
                    model: model.id.clone(),
                    effort: model.default_reasoning_effort.clone(),
                });
            }
        });

        row.add_row(&model_row);
        row.add_row(&effort_row);
    }
}

fn build_view() -> (PreferencesPage, PreferencesGroup, PreferencesGroup) {
    let view = PreferencesPage::new();
    let connected = PreferencesGroup::builder().title("Connected").build();
    let available = PreferencesGroup::builder().title("Available").build();
    view.add(&connected);
    view.add(&available);
    (view, connected, available)
}

fn logo(provider: &Provider) -> Image {
    let image = match Texture::from_bytes(&Bytes::from_static(provider.logo)) {
        Ok(texture) => Image::from_paintable(Some(&texture)),
        Err(e) => {
            log::warn!("failed to load {} logo: {e}", provider.id);
            Image::from_icon_name("application-x-executable-symbolic")
        },
    };
    image.set_pixel_size(40);
    image
}
