mod connection_dialog;
mod model;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Button, Image, Orientation, StringList, glib};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, ComboRow, ExpanderRow, PreferencesGroup,
    PreferencesPage, ResponseAppearance,
    gdk::Texture,
    glib::{Bytes, WeakRef},
    prelude::*,
};
use scry_core::{AppContext, Connection, ConnectorConnection, HealthStatus, ProviderId};
use tokio::task::JoinHandle;

use self::model::{Command, Model, Msg};
use crate::{
    helper::Clear,
    runtime::tokio_runtime,
    widgets::settings::{
        helper::{group_is_empty, placeholder, show_error_dialog, unhealthy_icon},
        services::connection_dialog::ConnectionDialog,
    },
};

pub(super) const CSS: &str = include_str!("style.css");

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
    connection_dialog: RefCell<Option<ConnectionDialog>>,
    connection_flow: RefCell<Option<JoinHandle<()>>>,
    app_context: Arc<AppContext>,
    window: WeakRef<ApplicationWindow>,
    model: RefCell<Model>,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl ServicesPage {
    pub(crate) fn new(app_context: Arc<AppContext>, window: &ApplicationWindow) -> Rc<Self> {
        let (view, connected_group, available_group) = build_view();

        let (dispatcher, mut receiver) = mpsc::unbounded::<Msg>();

        let service_page = Rc::new(Self {
            view,
            connected_group,
            available_group,
            connection_dialog: RefCell::new(None),
            connection_flow: RefCell::new(None),
            app_context,
            window: window.downgrade(),
            model: RefCell::new(Model::default()),
            dispatcher,
        });

        let service_event = Rc::downgrade(&service_page);
        glib::spawn_future_local(async move {
            while let Ok(msg) = receiver.recv().await {
                let Some(service) = service_event.upgrade() else {
                    break;
                };
                let commands = service.model.borrow_mut().update(msg);
                for command in commands {
                    service.run(command);
                }
            }
        });

        service_page
    }

    pub(crate) fn widget(&self) -> &PreferencesPage {
        &self.view
    }

    pub(crate) fn refresh(&self) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.available_connectors().await;
            let _ = dispatcher.unbounded_send(Msg::ConnectorsFetched(result));
        }));
    }

    fn persist_preference(&self, id: ProviderId, model: String, effort: String) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.set_model_preference(id, &model, &effort).await;
            let _ = dispatcher.unbounded_send(Msg::PreferenceSaveFinished(result));
        }));
    }

    fn show_disconnect_confirmation(&self, id: ProviderId) {
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

        let dispatcher = self.dispatcher.clone();
        dialog.connect_response(Some("disconnect"), move |_, _| {
            let _ = dispatcher.unbounded_send(Msg::DisconnectConfirmed(id));
        });
        dialog.present(Some(&window));
    }

    fn render(&self) {
        self.connected_group.clear();
        self.available_group.clear();

        let connectors = self.model.borrow().connectors.clone();
        for provider in PROVIDERS {
            let connection = connectors
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

    fn available_row(&self, provider: &'static Provider) -> ActionRow {
        let row = ActionRow::builder().title(provider.id.to_string()).build();
        row.add_prefix(&logo(provider));
        row.add_suffix(&self.connect_button(provider));
        row
    }

    fn connected_row(
        &self,
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

    fn connect_button(&self, provider: &'static Provider) -> Button {
        let button = Button::builder()
            .label("Connect")
            .valign(Align::Center)
            .css_classes(["suggested-action"])
            .build();

        let dispatcher = self.dispatcher.clone();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::ConnectClicked(provider.id));
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

        let dispatcher = self.dispatcher.clone();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::DisconnectClicked(provider.id));
        });
        button
    }

    fn add_picker_rows(&self, id: ProviderId, row: &ExpanderRow, conn: &ConnectorConnection) {
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

        let dispatcher = self.dispatcher.clone();
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
            let _ = dispatcher.unbounded_send(Msg::PreferenceChanged {
                id,
                model: model.id.clone(),
                effort: effort.clone(),
            });
        });

        let dispatcher = self.dispatcher.clone();
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

            let _ = dispatcher.unbounded_send(Msg::PreferenceChanged {
                id,
                model: model.id.clone(),
                effort: model.default_reasoning_effort.clone(),
            });
        });

        row.add_row(&model_row);
        row.add_row(&effort_row);
    }
}

impl ServicesPage {
    fn run(&self, command: Command) {
        match command {
            Command::Render => self.render(),
            Command::FetchConnectors => self.refresh(),
            Command::ShowDisconnectConfirmation(id) => self.show_disconnect_confirmation(id),
            Command::DisconnectProvider(id) => self.disconnect_provider(id),
            Command::PersistPreference { id, model, effort } => {
                self.persist_preference(id, model, effort)
            },
            Command::Warn(message) => log::warn!("{message}"),
            Command::ShowErrorDialog(message) => {
                if let Some(window) = self.window.upgrade() {
                    show_error_dialog(&window, "Service Connection Failed", &message);
                }
            },
            Command::InitConnection(provider_id) => self.init_connection(provider_id),
            Command::ShowConnectionDialog(provider_id) => {
                let Some(window) = self.window.upgrade() else {
                    return;
                };
                let dialog = ConnectionDialog::new(provider_id, self.dispatcher.clone());
                dialog.show(&window);
                self.connection_dialog.replace(Some(dialog));
            },
            Command::ShowLoading => self.with_dialog(|dialog| dialog.show_loading()),
            Command::ShowChallenge {
                verification_uri,
                user_code,
            } => {
                self.with_dialog(|dialog| dialog.show_challenge(verification_uri, &user_code));
            },
            Command::ShowManualInput {
                provider_id,
                instructions_url,
            } => self.with_dialog(|dialog| dialog.show_manual(provider_id, instructions_url)),
            Command::ShowOauth {
                provider_id,
                authorization_url,
            } => self.with_dialog(|dialog| dialog.show_oauth(provider_id, authorization_url)),
            Command::FinalizeConnection {
                provider_id,
                connection,
            } => self.finalize_connection(provider_id, connection),
            Command::ShowSuccess => self.with_dialog(|dialog| dialog.show_success()),
            Command::ShowError(error_msg) => {
                self.with_dialog(|dialog| dialog.show_error(&error_msg))
            },
            Command::CloseConnectionDialog => self.with_dialog(|dialog| dialog.close()),
            Command::DropConnectionDialog => {
                *self.connection_dialog.borrow_mut() = None;
                // Cancel the abandoned flow at its next await point; anything
                // it already sent is filtered by the model's provider guard.
                if let Some(flow) = self.connection_flow.borrow_mut().take() {
                    flow.abort();
                }
            },
        }
    }

    /// Run `f` on the open connection dialog; dialog commands arriving after
    /// it was dropped are ignored.
    fn with_dialog(&self, f: impl FnOnce(&ConnectionDialog)) {
        if let Some(dialog) = self.connection_dialog.borrow().as_ref() {
            f(dialog);
        }
    }

    /// Track the session's in-flight backend task, aborting any previous one.
    fn track_flow(&self, handle: JoinHandle<()>) {
        if let Some(old) = self.connection_flow.borrow_mut().replace(handle) {
            old.abort();
        }
    }

    fn disconnect_provider(&self, id: ProviderId) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.disconnect_connector(id).await;
            let _ = dispatcher.unbounded_send(Msg::DisconnectFinished(id, result));
        }));
    }

    fn init_connection(&self, provider_id: ProviderId) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        self.track_flow(tokio_runtime().spawn(async move {
            let result = app_context.init_connection(provider_id).await;
            let _ = dispatcher.unbounded_send(Msg::InitFinished(provider_id, result));
        }));
    }

    fn finalize_connection(&self, provider_id: ProviderId, connection: Connection) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        self.track_flow(tokio_runtime().spawn(async move {
            let result = app_context
                .finalize_connection(provider_id, connection)
                .await
                .map(|_| ());
            let _ = dispatcher.unbounded_send(Msg::FinalizeFinished(provider_id, result));
        }));
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
