mod model;
mod plugin_dialog;

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Button, Orientation, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, ApplicationWindow, ButtonRow, Dialog, ExpanderRow, HeaderBar, PreferencesGroup,
    PreferencesPage, Spinner, SpinnerPaintable, StatusPage, ToolbarView, prelude::*,
};
use scry_core::{
    AppContext, HealthStatus, McpServer, OAuthCallbackState, Plugin, PluginArgs, PluginType,
};
use tokio::task::JoinHandle;

use self::model::{Command, Msg, State};
use crate::{
    helper::Clear,
    runtime::tokio_runtime,
    widgets::settings::helper::{launch_url, show_error_dialog, unhealthy_icon},
};

pub(crate) struct PluginsPage {
    view: PreferencesPage,
    mcp_view: PreferencesGroup,
    add_mcp: ButtonRow,
    mcp_dialog: RefCell<Option<plugin_dialog::PluginDialog>>,
    oauth_dialog: RefCell<Option<(Dialog, glib::SignalHandlerId)>>,
    connection_flow: RefCell<Option<JoinHandle<()>>>,
    app_context: Arc<AppContext>,
    window: glib::WeakRef<ApplicationWindow>,
    model: RefCell<State>,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl PluginsPage {
    pub(crate) fn new(app_context: Arc<AppContext>, window: &ApplicationWindow) -> Rc<Self> {
        let view = PreferencesPage::new();

        // Native plugin support is not wired yet; the disabled add row also
        // serves as the empty state.
        let plugins = PreferencesGroup::builder().title("Plugins").build();
        plugins.add(
            &ButtonRow::builder()
                .title("Add Plugin…")
                .start_icon_name("list-add-symbolic")
                .sensitive(false)
                .tooltip_text("Native plugins are not supported yet.")
                .build(),
        );

        let mcp_view = PreferencesGroup::builder().title("MCP Servers").build();
        let add_mcp = ButtonRow::builder()
            .title("Add MCP Server…")
            .start_icon_name("list-add-symbolic")
            .build();

        view.add(&plugins);
        view.add(&mcp_view);

        let (dispatcher, mut receiver) = mpsc::unbounded::<Msg>();

        let add_mcp_dispatcher = dispatcher.clone();
        add_mcp.connect_activated(move |_| {
            let _ = add_mcp_dispatcher.unbounded_send(Msg::AddMcpClicked);
        });

        let plugin_page = Rc::new(Self {
            view,
            mcp_view,
            add_mcp,
            mcp_dialog: RefCell::new(None),
            oauth_dialog: RefCell::new(None),
            connection_flow: RefCell::new(None),
            app_context,
            window: window.downgrade(),
            model: RefCell::new(State::default()),
            dispatcher,
        });

        let service_event = Rc::downgrade(&plugin_page);
        glib::spawn_future_local(async move {
            while let Ok(msg) = receiver.recv().await {
                let Some(plugins) = service_event.upgrade() else {
                    break;
                };
                let commands = plugins.model.borrow_mut().update(msg);
                for command in commands {
                    plugins.run(command);
                }
            }
        });

        plugin_page
    }

    pub(crate) fn widget(&self) -> &PreferencesPage {
        &self.view
    }

    pub(crate) fn refresh(&self) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.list_mcps().await;
            let _ = dispatcher.unbounded_send(Msg::McpServersLoaded(result));
        }));
    }

    fn run(&self, command: Command) {
        match command {
            Command::RenderMcpServers => self.render_mcp_servers(),
            Command::LoadMcpServers => self.refresh(),
            Command::SaveMcpToggle(name, enabled) => self.save_mcp_toggle(name, enabled),
            Command::RemoveMcp(name) => self.remove_mcp(name),
            Command::OpenMcpDialog { config, taken } => self.open_mcp_dialog(config, taken),
            Command::SaveMcp(config) => self.init_mcp_connection(config),
            Command::UpdateMcp(config) => self.update_mcp(config),
            Command::ShowMcpDialogError(error_msg) => {
                if let Some(dialog) = self.mcp_dialog.borrow().as_ref() {
                    dialog.show_error(&error_msg);
                }
            },
            Command::CloseMcpDialog => {
                if let Some(dialog) = self.mcp_dialog.borrow_mut().take() {
                    dialog.hide();
                }
            },
            Command::ShowErrorDialog(message) => {
                if let Some(window) = self.window.upgrade() {
                    show_error_dialog(&window, "Plugin Operation Failed", &message);
                }
            },
            Command::LogWarning(message) => log::warn!("{message}"),
            Command::OpenOauthDialog(url) => {
                let Some(window) = self.window.upgrade() else {
                    return;
                };
                self.oauth_dialog.replace(Some(open_oauth_dialog(
                    &window,
                    &url,
                    self.dispatcher.clone(),
                )));
            },
            Command::FinalizeMcp { config, state } => self.finalize_mcp_connection(config, state),
            Command::CloseOauthDialog => {
                if let Some((dialog, closed)) = self.oauth_dialog.borrow_mut().take() {
                    // Disconnect first: this close is not a cancellation.
                    dialog.disconnect(closed);
                    dialog.force_close();
                }
            },
            Command::AbortMcpConnection => {
                // The popup already closed itself; drop the stale handle.
                self.oauth_dialog.borrow_mut().take();
                if let Some(flow) = self.connection_flow.borrow_mut().take() {
                    flow.abort();
                }
            },
        }
    }

    fn save_mcp_toggle(&self, name: String, enabled: bool) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.toggle_plugin(&name, !enabled).await;
            let _ = dispatcher.unbounded_send(Msg::McpToggleFinished(result));
        }));
    }

    fn init_mcp_connection(&self, config: Plugin) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context
                .init_mcp_connection(config.clone())
                .await
                .map(|state| state.map(Box::new));
            let _ = dispatcher.unbounded_send(Msg::McpInitFinished { config, result });
        }));
    }

    fn update_mcp(&self, config: Plugin) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.update_plugin(PluginType::Mcp, config).await;
            let _ = dispatcher.unbounded_send(Msg::McpSaveFinished(result));
        }));
    }

    fn finalize_mcp_connection(&self, config: Plugin, state: Option<Box<OAuthCallbackState>>) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        let flow = tokio_runtime().spawn(async move {
            let result = app_context
                .finalize_mcp_connection(config, state.map(|state| *state))
                .await;
            let _ = dispatcher.unbounded_send(Msg::McpSaveFinished(result));
        });
        if let Some(old) = self.connection_flow.borrow_mut().replace(flow) {
            old.abort();
        }
    }

    fn remove_mcp(&self, name: String) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.remove_plugin(PluginType::Mcp, &name).await;
            let _ = dispatcher.unbounded_send(Msg::RemoveMcpFinished(result));
        }));
    }

    /// Open the plugin dialog: `None` adds a new server, `Some` edits it.
    fn open_mcp_dialog(&self, config: Option<Plugin>, taken: HashSet<String>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let dialog = plugin_dialog::PluginDialog::new(config, taken, self.dispatcher.clone());
        dialog.show(&window);
        self.mcp_dialog.replace(Some(dialog));
    }

    fn render_mcp_servers(&self) {
        self.mcp_view.clear();
        let servers = self.model.borrow().servers.clone();
        for server in &servers {
            self.mcp_view.add(&self.mcp_row(server));
        }
        // The add row sits last so new servers appear above it.
        self.mcp_view.add(&self.add_mcp);
    }

    fn mcp_row(&self, server: &McpServer) -> ExpanderRow {
        let config = &server.config;
        let row = ExpanderRow::builder()
            .title(&config.name)
            .subtitle(&server.description)
            .build();

        for (title, value) in config_props(config) {
            if value.is_empty() {
                continue;
            }
            row.add_row(
                &ActionRow::builder()
                    .title(title)
                    .subtitle(&value)
                    .css_classes(["property"])
                    .build(),
            );
        }

        let actions = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .valign(Align::Center)
            .build();
        match server.status {
            HealthStatus::Running => actions.append(&self.toggle_switch(config)),
            HealthStatus::Unhealthy => actions.append(&unhealthy_icon(server.error.as_deref())),
            HealthStatus::Starting => actions.append(&starting_spinner()),
        }
        actions.append(&self.edit_button(config));
        actions.append(&self.remove_button(config));
        row.add_suffix(&actions);
        row
    }

    fn toggle_switch(&self, config: &Plugin) -> Switch {
        let switch = Switch::builder()
            .active(!config.disabled)
            .valign(Align::Center)
            .tooltip_text("Enable or disable the plugin")
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = config.name.clone();
        switch.connect_state_set(move |_, state| {
            let _ = dispatcher.unbounded_send(Msg::McpToggleChanged(name.clone(), state));
            glib::Propagation::Proceed
        });
        switch
    }

    fn edit_button(&self, config: &Plugin) -> Button {
        let button = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = config.name.clone();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::EditMcpClicked(name.clone()));
        });
        button
    }

    fn remove_button(&self, config: &Plugin) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = config.name.clone();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::RemoveMcpClicked(name.clone()));
        });
        button
    }
}

fn starting_spinner() -> Spinner {
    Spinner::builder()
        .tooltip_text("Connecting…")
        .valign(Align::Center)
        .build()
}

/// Present the "waiting for authorization" popup. Dismissing it cancels the
/// authorization; the page closes it once the connection settles, using the
/// returned handler id to disconnect that report first.
fn open_oauth_dialog(
    parent: &impl IsA<gtk4::Widget>,
    auth_url: &str,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> (Dialog, glib::SignalHandlerId) {
    launch_url(auth_url);

    let status = StatusPage::builder()
        .title("Waiting for Authorization")
        .description("Complete the sign-in in your browser.")
        .build();
    status.set_paintable(Some(&SpinnerPaintable::new(Some(&status))));

    // Fallback in case the browser did not open on its own.
    let open = Button::builder()
        .label("Open the authorization page")
        .halign(Align::Center)
        .tooltip_text(auth_url)
        .css_classes(["link"])
        .build();
    let url = auth_url.to_string();
    open.connect_clicked(move |_| launch_url(&url));
    status.set_child(Some(&open));

    let view = ToolbarView::builder().content(&status).build();
    view.add_top_bar(&HeaderBar::new());

    let dialog = Dialog::builder()
        .title("Connect Plugin")
        .content_width(420)
        .child(&view)
        .build();
    let closed = dialog.connect_closed(move |_| {
        let _ = dispatcher.unbounded_send(Msg::OauthDialogClosed);
    });
    dialog.present(Some(parent));
    (dialog, closed)
}

fn config_props(config: &Plugin) -> Vec<(&'static str, String)> {
    let mut props = match &config.args {
        PluginArgs::Local { command, args } => vec![
            ("Transport", "Local command".to_string()),
            ("Command", command.clone()),
            ("Arguments", serde_json::to_string(args).unwrap_or_default()),
        ],
        PluginArgs::Remote { url, requires_auth } => vec![
            ("Transport", "Remote server".to_string()),
            ("URL", url.clone()),
            (
                "Requires authentication",
                if *requires_auth { "Yes" } else { "No" }.to_string(),
            ),
        ],
    };
    props.push(("Timeout", format!("{}s", config.timeout)));
    if !config.env.is_empty() {
        let mut env: Vec<String> = config.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        env.sort();
        props.push(("Environment", env.join("\n")));
    }
    props
}
