mod model;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use gtk4::{Align, Box as GtkBox, Button, Orientation, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, ApplicationWindow, ButtonRow, ExpanderRow, PreferencesGroup, PreferencesPage,
    Spinner, prelude::*,
};
use scry_core::{AppContext, HealthStatus, McpServer, Plugin, PluginArgs, PluginType};

use self::model::{Command, Msg, State};
use crate::{
    helper::Clear,
    runtime,
    widgets::settings::{
        helper::{show_error_dialog, unhealthy_icon},
        plugins::{
            modal,
            modal::{SaveFinished, SavePlugin},
        },
    },
};

pub(crate) struct PluginsPage {
    view: PreferencesPage,
    mcp_view: PreferencesGroup,
    add_mcp: ButtonRow,
    app_context: Arc<AppContext>,
    window: glib::WeakRef<ApplicationWindow>,
    state: RefCell<State>,
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

        let this = Rc::new(Self {
            view,
            mcp_view,
            add_mcp,
            app_context,
            window: window.downgrade(),
            state: RefCell::new(State::default()),
        });

        let weak = Rc::downgrade(&this);
        this.add_mcp.connect_activated(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::AddMcpClicked);
            }
        });

        this
    }

    pub(crate) fn widget(&self) -> &PreferencesPage {
        &self.view
    }

    pub(crate) fn refresh(self: &Rc<Self>) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.list_mcps().await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::McpServersLoaded(result));
                }
            },
        );
    }

    fn dispatch(self: &Rc<Self>, msg: Msg) {
        let commands = self.state.borrow_mut().update(msg);
        for command in commands {
            self.run(command);
        }
    }

    fn run(self: &Rc<Self>, command: Command) {
        match command {
            Command::RenderMcpServers => self.render_mcp_servers(),
            Command::LoadMcpServers => self.refresh(),
            Command::SaveMcpToggle(name, enabled) => self.save_mcp_toggle(name, enabled),
            Command::RemoveMcp(name) => self.remove_mcp(name),
            Command::OpenAddMcpDialog => self.open_add_mcp_dialog(),
            Command::OpenEditMcpDialog(config) => self.open_edit_mcp_dialog(config),
            Command::ShowErrorDialog(message) => {
                if let Some(window) = self.window.upgrade() {
                    show_error_dialog(&window, "Plugin Operation Failed", &message);
                }
            },
            Command::LogWarning(message) => log::warn!("{message}"),
        }
    }

    fn save_mcp_toggle(self: &Rc<Self>, name: String, enabled: bool) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.toggle_plugin(&name, !enabled).await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::McpToggleFinished(result));
                }
            },
        );
    }

    fn remove_mcp(self: &Rc<Self>, name: String) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.remove_plugin(PluginType::Mcp, &name).await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::RemoveMcpFinished(result));
                }
            },
        );
    }

    fn open_add_mcp_dialog(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let taken = self.state.borrow().names.clone();
        modal::open(&window, taken, None, self.save_plugin(false));
    }

    fn open_edit_mcp_dialog(self: &Rc<Self>, config: Plugin) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        // The server keeps its own name, so allow it.
        let mut taken = self.state.borrow().names.clone();
        taken.remove(&config.name);
        modal::open(&window, taken, Some(config), self.save_plugin(true));
    }

    fn save_plugin(self: &Rc<Self>, editing: bool) -> SavePlugin {
        let weak = Rc::downgrade(self);
        Rc::new(move |config, save_finished| {
            if let Some(this) = weak.upgrade() {
                this.persist(editing, config, save_finished);
            }
        })
    }

    fn persist(self: &Rc<Self>, editing: bool, config: Plugin, save_finished: SaveFinished) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move {
                if editing {
                    app_context.update_plugin(PluginType::Mcp, config).await
                } else {
                    app_context.add_mcp(config).await
                }
            },
            move |result| match result {
                Ok(()) => {
                    save_finished(Ok(()));
                    if let Some(this) = weak.upgrade() {
                        this.dispatch(Msg::ReloadMcpServersRequested);
                    }
                },
                Err(e) => save_finished(Err(e.to_string())),
            },
        );
    }

    fn render_mcp_servers(self: &Rc<Self>) {
        self.mcp_view.clear();
        let state = self.state.borrow();
        for server in &state.servers {
            self.mcp_view.add(&self.mcp_row(server));
        }
        // The add row sits last so new servers appear above it.
        self.mcp_view.add(&self.add_mcp);
    }

    fn mcp_row(self: &Rc<Self>, server: &McpServer) -> ExpanderRow {
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

    fn toggle_switch(self: &Rc<Self>, config: &Plugin) -> Switch {
        let switch = Switch::builder()
            .active(!config.disabled)
            .valign(Align::Center)
            .tooltip_text("Enable or disable the plugin")
            .build();

        let weak = Rc::downgrade(self);
        let name = config.name.clone();
        switch.connect_state_set(move |_, state| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::McpToggleChanged(name.clone(), state));
            }
            glib::Propagation::Proceed
        });
        switch
    }

    fn edit_button(self: &Rc<Self>, config: &Plugin) -> Button {
        let button = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let weak = Rc::downgrade(self);
        let name = config.name.clone();
        button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::EditMcpClicked(name.clone()));
            }
        });
        button
    }

    fn remove_button(self: &Rc<Self>, config: &Plugin) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let weak = Rc::downgrade(self);
        let name = config.name.clone();
        button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::RemoveMcpClicked(name.clone()));
            }
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
