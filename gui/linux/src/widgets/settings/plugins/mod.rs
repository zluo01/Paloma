mod model;
mod plugin_dialog;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, Switch, ToggleButton, Widget, glib,
    prelude::*,
};
use libadwaita::{
    ActionRow, ApplicationWindow, ButtonRow, Dialog, ExpanderRow, HeaderBar, PreferencesGroup,
    PreferencesPage, PreferencesRow, Spinner, SpinnerPaintable, StatusPage, ToolbarView,
    prelude::*,
};
use paloma_core::{
    AppContext, CapabilityFacet, CapabilityInfo, ExtensionInfo, HealthStatus, McpPluginInfo,
    OAuthCallbackState, Plugin, PluginType, ProviderInfo,
};
use tokio::task::JoinHandle;

use self::model::{Command, Msg, State};
use crate::{
    helper::Clear,
    runtime::tokio_runtime,
    widgets::settings::{
        helper::{launch_url, show_error_dialog, unhealthy_icon},
        plugins::model::{GeneralPluginMsg, McpPluginMsg},
    },
};

pub(super) const CSS: &str = include_str!("style.css");

pub(crate) struct PluginsPage {
    view: PreferencesPage,
    extension_view: PreferencesGroup,
    provider_view: PreferencesGroup,
    mcp_view: PreferencesGroup,
    add_extension: ButtonRow,
    add_provider: ButtonRow,
    add_mcp: ButtonRow,
    dialog: RefCell<Option<plugin_dialog::PluginDialog>>,
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

        let extension_view = PreferencesGroup::builder().title("Extensions").build();
        let add_extension = ButtonRow::builder()
            .title("Add Extension Plugin…")
            .start_icon_name("list-add-symbolic")
            .build();

        let provider_view = PreferencesGroup::builder().title("Providers").build();
        let add_provider = ButtonRow::builder()
            .title("Add Provider Plugin…")
            .start_icon_name("list-add-symbolic")
            .build();

        let mcp_view = PreferencesGroup::builder().title("MCP Servers").build();
        let add_mcp = ButtonRow::builder()
            .title("Add MCP Server…")
            .start_icon_name("list-add-symbolic")
            .build();

        view.add(&extension_view);
        view.add(&provider_view);
        view.add(&mcp_view);

        let (dispatcher, mut receiver) = mpsc::unbounded::<Msg>();

        add_extension.connect_activated({
            let dispatcher = dispatcher.clone();
            move |_| {
                let _ = dispatcher.unbounded_send(Msg::General(
                    GeneralPluginMsg::AddPluginClicked(PluginType::Extension),
                ));
            }
        });

        add_provider.connect_activated({
            let dispatcher = dispatcher.clone();
            move |_| {
                let _ = dispatcher.unbounded_send(Msg::General(
                    GeneralPluginMsg::AddPluginClicked(PluginType::Provider),
                ));
            }
        });

        add_mcp.connect_activated({
            let dispatcher = dispatcher.clone();
            move |_| {
                let _ = dispatcher.unbounded_send(Msg::General(
                    GeneralPluginMsg::AddPluginClicked(PluginType::Mcp),
                ));
            }
        });

        let plugin_page = Rc::new(Self {
            view,
            extension_view,
            provider_view,
            mcp_view,
            add_extension,
            add_provider,
            add_mcp,
            dialog: RefCell::new(None),
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
        self.refresh_extensions();
        self.refresh_mcp_servers();
        self.refresh_providers();
    }

    fn refresh_extensions(&self) {
        drop(tokio_runtime().spawn({
            let app_context = self.app_context.clone();
            let dispatcher = self.dispatcher.clone();
            async move {
                let result = app_context.list_extension_plugins().await;
                let _ = dispatcher.unbounded_send(Msg::ExtensionLoaded(result));
            }
        }));
    }

    fn refresh_providers(&self) {
        drop(tokio_runtime().spawn({
            let app_context = self.app_context.clone();
            let dispatcher = self.dispatcher.clone();
            async move {
                let result = app_context.list_provider_plugins().await;
                let _ = dispatcher.unbounded_send(Msg::ProviderLoaded(result));
            }
        }));
    }

    fn refresh_mcp_servers(&self) {
        drop(tokio_runtime().spawn({
            let app_context = self.app_context.clone();
            let dispatcher = self.dispatcher.clone();
            async move {
                let result = app_context.list_mcps().await;
                let _ = dispatcher.unbounded_send(Msg::McpServersLoaded(result));
            }
        }));
    }

    fn run(&self, command: Command) {
        match command {
            Command::RenderExtensions => self.render_extension_plugins(),
            Command::RenderMcpServers => self.render_mcp_servers(),
            Command::RenderProviderPlugins => self.render_provider_plugins(),
            Command::LoadExtensions => self.refresh_extensions(),
            Command::LoadProviderPlugins => self.refresh_providers(),
            Command::LoadMcpServers => self.refresh_mcp_servers(),
            Command::TogglePlugin(plugin_type, name, enabled) => {
                self.toggle_plugin(plugin_type, name, enabled)
            },
            Command::ToggleCapability {
                plugin_type,
                plugin,
                capability,
                facet,
                enabled,
            } => self.toggle_capability(plugin_type, plugin, capability, facet, enabled),
            Command::RemovePlugin(plugin_type, name) => self.remove_plugin(plugin_type, name),
            Command::OpenPluginDialog {
                plugin_type,
                config,
                taken,
            } => self.open_plugin_dialog(plugin_type, config, taken),
            Command::SavePlugin(plugin_type, config) => self.save_plugin(plugin_type, config),
            Command::UpdatePlugin(plugin_type, config) => self.update_plugin(plugin_type, config),
            Command::ShowPluginDialogError(error_msg) => {
                if let Some(dialog) = self.dialog.borrow().as_ref() {
                    dialog.show_error(&error_msg);
                }
            },
            Command::ClosePluginDialog => {
                if let Some(dialog) = self.dialog.borrow_mut().take() {
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

    fn toggle_plugin(&self, plugin_type: PluginType, name: String, enabled: bool) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.toggle_plugin(&name, !enabled).await;
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::SwitchToggledFinish(
                plugin_type.clone(),
                result,
            )));
        }));
    }

    fn toggle_capability(
        &self,
        plugin_type: PluginType,
        plugin: String,
        capability: String,
        facet: CapabilityFacet,
        enabled: bool,
    ) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context
                .toggle_capability(&plugin, &capability, facet, !enabled)
                .await;
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::SwitchToggledFinish(
                plugin_type,
                result,
            )));
        }));
    }

    fn save_plugin(&self, plugin_type: PluginType, config: Plugin) {
        match plugin_type {
            PluginType::Extension | PluginType::Provider => {
                let app_context = self.app_context.clone();
                let dispatcher = self.dispatcher.clone();
                let flow = tokio_runtime().spawn(async move {
                    let result = match plugin_type {
                        PluginType::Extension => app_context.add_extension_plugin(config).await,
                        _ => app_context.add_provider_plugin(config).await,
                    };
                    let _ = dispatcher.unbounded_send(Msg::General(
                        GeneralPluginMsg::PluginSaveFinished(plugin_type, result),
                    ));
                });
                if let Some(old) = self.connection_flow.borrow_mut().replace(flow) {
                    old.abort();
                }
            },
            PluginType::Mcp => self.init_mcp_connection(config),
        }
    }

    fn init_mcp_connection(&self, config: Plugin) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context
                .init_mcp_connection(config.clone())
                .await
                .map(|state| state.map(Box::new));
            let _ = dispatcher.unbounded_send(Msg::McpPlugin(McpPluginMsg::McpInitFinished {
                config,
                result,
            }));
        }));
    }

    fn finalize_mcp_connection(&self, config: Plugin, state: Option<Box<OAuthCallbackState>>) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        let flow = tokio_runtime().spawn(async move {
            let result = app_context
                .finalize_mcp_connection(config, state.map(|state| *state))
                .await;
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::PluginSaveFinished(
                PluginType::Mcp,
                result,
            )));
        });
        if let Some(old) = self.connection_flow.borrow_mut().replace(flow) {
            old.abort();
        }
    }

    fn update_plugin(&self, plugin_type: PluginType, config: Plugin) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.update_plugin(plugin_type.clone(), config).await;
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::PluginSaveFinished(
                plugin_type,
                result,
            )));
        }));
    }

    fn remove_plugin(&self, plugin_type: PluginType, name: String) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(tokio_runtime().spawn(async move {
            let result = app_context.remove_plugin(plugin_type.clone(), &name).await;
            let _ = dispatcher.unbounded_send(Msg::General(
                GeneralPluginMsg::RemovePluginFinished(plugin_type, result),
            ));
        }));
    }

    /// Open the plugin dialog: `None` adds a new server, `Some` edits it.
    fn open_plugin_dialog(
        &self,
        plugin_type: PluginType,
        config: Option<Plugin>,
        taken: HashSet<String>,
    ) {
        let Some(window) = self.window.upgrade() else {
            return;
        };

        let dialog = match plugin_type {
            PluginType::Extension | PluginType::Provider => {
                plugin_dialog::PluginDialog::new_local_plugin_dialog(
                    plugin_type,
                    config,
                    self.dispatcher.clone(),
                )
            },
            PluginType::Mcp => {
                plugin_dialog::PluginDialog::new_mcp_dialog(config, taken, self.dispatcher.clone())
            },
        };

        dialog.show(&window);
        self.dialog.replace(Some(dialog));
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

    fn render_provider_plugins(&self) {
        self.provider_view.clear();
        let providers = self.model.borrow().providers.clone();
        for provider in &providers {
            self.provider_view.add(&self.provider_row(provider));
        }
        self.provider_view.add(&self.add_provider);
    }

    fn render_extension_plugins(&self) {
        self.extension_view.clear();
        let extensions = self.model.borrow().extensions.clone();
        for extension in &extensions {
            self.extension_view.add(&self.extension_row(extension));
        }
        self.extension_view.add(&self.add_extension);
    }

    fn extension_row(&self, extension: &ExtensionInfo) -> Widget {
        let subtitle = [
            extension
                .author
                .as_deref()
                .map(|author| format!("Author: {author}")),
            extension
                .homepage
                .as_deref()
                .map(|homepage| format!("Homepage: {homepage}")),
            Some(extension.description.clone()).filter(|description| !description.is_empty()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");

        let row = ExpanderRow::builder()
            .title(&extension.name)
            .subtitle(&subtitle)
            .build();

        let switch = extension
            .config
            .as_ref()
            .map(|config| self.toggle_switch(PluginType::Extension, config));

        fill_on_expand(&row, &extension.capabilities, {
            let dispatcher = self.dispatcher.clone();
            let plugin = extension.name.clone();
            let switch = switch.clone();
            move |capability: &CapabilityInfo| {
                capability_row(&plugin, switch.as_ref(), capability, &dispatcher)
            }
        });

        let actions = plugin_actions(
            extension.status,
            extension.error.as_deref(),
            switch.map(|switch| switch.upcast()),
        );
        if extension.config.is_some() && extension.status != HealthStatus::Starting {
            actions.append(&self.edit_button(PluginType::Extension, &extension.name));
            actions.append(&self.remove_button(PluginType::Extension, &extension.name));
        }
        row.add_suffix(&actions);
        row.upcast()
    }

    fn provider_row(&self, provider: &ProviderInfo) -> Widget {
        let row = ActionRow::builder()
            .title(&provider.name)
            .subtitle(&provider.description)
            .build();

        let actions = plugin_actions(provider.status, provider.error.as_deref(), None);
        if provider.config.is_some() && provider.status != HealthStatus::Starting {
            actions.append(&self.edit_button(PluginType::Provider, &provider.name));
            actions.append(&self.remove_button(PluginType::Provider, &provider.name));
        }
        row.add_suffix(&actions);
        row.upcast()
    }

    fn mcp_row(&self, server: &McpPluginInfo) -> Widget {
        let config = &server.config;
        let row = ExpanderRow::builder()
            .title(&config.name)
            .subtitle(&server.description)
            .build();

        let switch = self.toggle_switch(PluginType::Mcp, config);

        fill_on_expand(&row, &server.tools, {
            let dispatcher = self.dispatcher.clone();
            let server = config.name.clone();
            let plugin_switch = switch.clone();
            move |tool: &CapabilityInfo| tool_row(&server, &plugin_switch, tool, &dispatcher)
        });

        let actions = plugin_actions(
            server.status,
            server.error.as_deref(),
            Some(switch.upcast()),
        );

        if server.status != HealthStatus::Starting {
            actions.append(&self.edit_button(PluginType::Mcp, &config.name));
            actions.append(&self.remove_button(PluginType::Mcp, &config.name));
        }
        row.add_suffix(&actions);
        row.upcast()
    }

    fn toggle_switch(&self, plugin_type: PluginType, config: &Plugin) -> Switch {
        let switch = Switch::builder()
            .active(!config.disabled)
            .valign(Align::Center)
            .tooltip_text("Enable or disable the plugin")
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = config.name.clone();
        switch.connect_state_set(move |_, state| {
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::ToggleSwitch(
                plugin_type.clone(),
                name.clone(),
                state,
            )));
            glib::Propagation::Proceed
        });
        switch
    }

    fn edit_button(&self, plugin_type: PluginType, name: &str) -> Button {
        let button = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = name.to_owned();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::EditPluginClicked(
                plugin_type.clone(),
                name.clone(),
            )));
        });
        button
    }

    fn remove_button(&self, plugin_type: PluginType, name: &str) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let dispatcher = self.dispatcher.clone();
        let name = name.to_owned();
        button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::RemovePluginClicked(
                plugin_type.clone(),
                name.clone(),
            )));
        });
        button
    }
}

fn plugin_actions(status: HealthStatus, error: Option<&str>, running: Option<Widget>) -> GtkBox {
    let actions = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    match status {
        HealthStatus::Running => {
            if let Some(running) = running {
                actions.append(&running);
            }
        },
        HealthStatus::Unhealthy => actions.append(&unhealthy_icon(error)),
        HealthStatus::Starting => actions.append(&starting_spinner()),
    }
    actions
}

fn starting_spinner() -> Spinner {
    Spinner::builder()
        .tooltip_text("Connecting…")
        .valign(Align::Center)
        .build()
}

fn capability_row(
    plugin: &str,
    plugin_switch: Option<&Switch>,
    capability: &CapabilityInfo,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) -> PreferencesRow {
    let title_line = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    title_line.append(
        &Label::builder()
            .label(&capability.id)
            .css_classes(["title"])
            .halign(Align::Start)
            .xalign(0.0)
            .build(),
    );
    for &(facet, disabled) in &capability.facets {
        title_line.append(&facet_chip(
            plugin,
            &capability.id,
            facet,
            disabled,
            plugin_switch,
            dispatcher,
        ));
    }

    let title_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .css_classes(["title"])
        .valign(Align::Center)
        .hexpand(true)
        .build();
    title_box.append(&title_line);
    title_box.append(
        &Label::builder()
            .label(&capability.description)
            .css_classes(["subtitle"])
            .halign(Align::Start)
            .xalign(0.0)
            .wrap(true)
            .build(),
    );

    let header = GtkBox::builder()
        .css_classes(["header"])
        .valign(Align::Center)
        .hexpand(false)
        .build();
    header.append(&title_box);

    let row = PreferencesRow::builder()
        .title(&capability.id)
        .activatable(false)
        .build();
    row.set_child(Some(&header));
    row
}

fn tool_row(
    server: &str,
    plugin_switch: &Switch,
    tool: &CapabilityInfo,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) -> ActionRow {
    let row = ActionRow::builder()
        .title(&tool.id)
        .subtitle(&tool.description)
        .subtitle_lines(0)
        .build();

    let disabled = tool.facets.iter().any(|&(_, disabled)| disabled);
    let switch = Switch::builder()
        .active(!disabled)
        .valign(Align::Center)
        .build();
    plugin_switch
        .bind_property("active", &switch, "sensitive")
        .sync_create()
        .build();
    switch.connect_state_set({
        let dispatcher = dispatcher.clone();
        let server = server.to_string();
        let capability = tool.id.clone();
        move |_, state| {
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::ToggleCapability {
                plugin_type: PluginType::Mcp,
                plugin: server.clone(),
                capability: capability.clone(),
                facet: CapabilityFacet::Mcp,
                enabled: state,
            }));
            glib::Propagation::Proceed
        }
    });
    row.add_suffix(&switch);
    row
}

fn facet_chip(
    plugin: &str,
    capability: &str,
    facet: CapabilityFacet,
    disabled: bool,
    plugin_switch: Option<&Switch>,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) -> ToggleButton {
    let chip = ToggleButton::builder()
        .label(facet_label(&facet))
        .active(!disabled)
        .valign(Align::Center)
        .css_classes(["paloma-capability-badge"])
        .build();
    if let Some(plugin_switch) = plugin_switch {
        plugin_switch
            .bind_property("active", &chip, "sensitive")
            .sync_create()
            .build();
    }
    chip.connect_toggled({
        let dispatcher = dispatcher.clone();
        let plugin = plugin.to_string();
        let capability = capability.to_string();
        move |chip| {
            let _ = dispatcher.unbounded_send(Msg::General(GeneralPluginMsg::ToggleCapability {
                plugin_type: PluginType::Extension,
                plugin: plugin.clone(),
                capability: capability.clone(),
                facet,
                enabled: chip.is_active(),
            }));
        }
    });
    chip
}

fn facet_label(facet: &CapabilityFacet) -> &'static str {
    match facet {
        CapabilityFacet::Search => "Search",
        CapabilityFacet::Tool | CapabilityFacet::Mcp => "Tool",
    }
}

/// lazy load the sublist
fn fill_on_expand<T, W>(row: &ExpanderRow, items: &[T], build: impl Fn(&T) -> W + 'static)
where
    T: Clone + 'static,
    W: IsA<Widget>,
{
    if items.is_empty() {
        return;
    }

    let items = items.to_vec();
    // The signal fires on collapse and on every later expand too, so the rows
    // are built exactly once.
    let filled = Cell::new(false);
    row.connect_expanded_notify(move |row| {
        if !row.is_expanded() || filled.replace(true) {
            return;
        }
        for item in &items {
            row.add_row(&build(item));
        }
    });
}

/// Present the "waiting for authorization" popup. Dismissing it cancels the
/// authorization; the page closes it once the connection settles, using the
/// returned handler id to disconnect that report first.
fn open_oauth_dialog(
    parent: &impl IsA<Widget>,
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
        let _ = dispatcher.unbounded_send(Msg::McpPlugin(McpPluginMsg::OauthDialogClosed));
    });
    dialog.present(Some(parent));
    (dialog, closed)
}
