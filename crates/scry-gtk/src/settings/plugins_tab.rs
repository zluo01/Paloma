// Plugins tab — native plugins (none yet) and user-managed MCP servers.

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use adw::{prelude::*, ActionRow, ButtonRow, ExpanderRow};
use gtk4::{
    glib, AlertDialog, Align, Box as GtkBox, Button, Image, ListBox, Orientation, Switch, Widget,
    Window,
};
use libadwaita as adw;
use scry_capability::HealthStatus;
use scry_controller::{McpServer, Plugin, PluginArgs, PluginType};
use scry_core::AppContext;

use super::plugin_modal::{self, SubmitDone};
use crate::runtime;

/// Build the Plugins tab; `window` parents the dialogs it opens.
pub fn build(app: Arc<AppContext>, window: Window) -> Widget {
    // Both lists end in an add row, which doubles as the empty state.
    let plugins = super::section("Plugins", "");
    plugins.placeholder.set_visible(false);
    plugins.list.append(
        &ButtonRow::builder()
            .title("Add Plugin…")
            .start_icon_name("list-add-symbolic")
            .sensitive(false)
            .tooltip_text("Native plugins are not supported yet.")
            .build(),
    );

    let mcp = super::section("MCP Servers", "");
    mcp.placeholder.set_visible(false);

    let add = ButtonRow::builder()
        .title("Add MCP Server…")
        .start_icon_name("list-add-symbolic")
        .build();
    mcp.list.append(&add);

    let servers = McpSection {
        list: mcp.list.clone(),
        add: add.clone(),
        rows: Rc::new(RefCell::new(Vec::new())),
        names: Rc::new(RefCell::new(HashSet::new())),
        app,
        window,
    };
    refresh(servers.clone());

    {
        let servers = servers.clone();
        add.connect_activated(move |_| {
            let on_submit: Rc<dyn Fn(Plugin, SubmitDone)> = {
                let servers = servers.clone();
                Rc::new(move |config, done| submit(servers.clone(), false, config, done))
            };
            let taken = servers.names.borrow().clone();
            plugin_modal::open(&servers.window, taken, None, on_submit);
        });
    }

    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .valign(Align::Start)
        .build();
    page.append(&plugins.root);
    page.append(&mcp.root);
    page.upcast()
}

/// The MCP server list plus the context its row handlers need. Cheap to
/// clone into signal closures.
#[derive(Clone)]
struct McpSection {
    list: ListBox,
    add: ButtonRow,
    /// Server rows currently in the list, so a refresh can remove them.
    rows: Rc<RefCell<Vec<Widget>>>,
    /// Names of the listed servers; the add dialog rejects duplicates.
    names: Rc<RefCell<HashSet<String>>>,
    app: Arc<AppContext>,
    window: Window,
}

/// Re-fetch the configured servers and rebuild the list.
fn refresh(servers: McpSection) {
    glib::MainContext::default().spawn_local(async move {
        let app = servers.app.clone();
        match runtime::spawn(async move { app.plugin.list_mcps().await }).await {
            Ok(list) => {
                *servers.names.borrow_mut() = list.iter().map(|s| s.config.name.clone()).collect();
                for row in servers.rows.borrow_mut().drain(..) {
                    servers.list.remove(&row);
                }
                for server in list {
                    let row: Widget = mcp_row(&servers, &server).upcast();
                    servers.list.insert(&row, servers.add.index());
                    servers.rows.borrow_mut().push(row);
                }
            },
            Err(e) => log::warn!("list_mcps failed: {e}"),
        }
    });
}

/// One MCP server: collapsed shows name, description, and the controls;
/// expanding reveals the stored configuration.
fn mcp_row(servers: &McpSection, server: &McpServer) -> ExpanderRow {
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
        HealthStatus::Running => actions.append(&toggle(servers, config)),
        HealthStatus::Unhealthy => actions.append(
            &Image::builder()
                .icon_name("dialog-information-symbolic")
                .tooltip_text(server.error.as_deref().unwrap_or("unknown error"))
                .valign(Align::Center)
                .build(),
        ),
    }
    actions.append(&edit_button(servers, config));
    actions.append(&remove_button(servers, config));
    row.add_suffix(&actions);
    row
}

/// Re-opens the add dialog pre-filled with this server's configuration;
/// saving replaces the old registration.
fn edit_button(servers: &McpSection, config: &Plugin) -> Button {
    let button = Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text("Edit plugin")
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build();

    let servers = servers.clone();
    let config = config.clone();
    button.connect_clicked(move |_| {
        let on_submit: Rc<dyn Fn(Plugin, SubmitDone)> = {
            let servers = servers.clone();
            Rc::new(move |config, done| submit(servers.clone(), true, config, done))
        };
        // Keeping its own name is not a duplicate.
        let mut taken = servers.names.borrow().clone();
        taken.remove(&config.name);
        plugin_modal::open(&servers.window, taken, Some(config.clone()), on_submit);
    });
    button
}

fn config_props(config: &Plugin) -> Vec<(&'static str, String)> {
    let mut props = match &config.args {
        PluginArgs::Local { command, args } => vec![
            ("Transport", "Local command".to_string()),
            ("Command", command.clone()),
            ("Arguments", args.join(" ")),
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

/// Enabled/disabled switch; persists the new state either way.
fn toggle(servers: &McpSection, config: &Plugin) -> Switch {
    let switch = Switch::builder()
        .active(!config.disabled)
        .valign(Align::Center)
        .tooltip_text("Enable or disable the plugin")
        .build();

    let servers = servers.clone();
    let name = config.name.clone();
    switch.connect_state_set(move |_, state| {
        let servers = servers.clone();
        let name = name.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = runtime::spawn({
                let app = servers.app.clone();
                async move { app.plugin.toggle_plugin(&name, !state).await }
            })
            .await;
            if let Err(e) = result {
                show_error(&servers.window, &e.to_string());
            }
        });
        glib::Propagation::Proceed
    });
    switch
}

fn remove_button(servers: &McpSection, config: &Plugin) -> Button {
    let button = Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Remove plugin")
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build();

    let servers = servers.clone();
    let name = config.name.clone();
    button.connect_clicked(move |_| {
        let servers = servers.clone();
        let name = name.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = runtime::spawn({
                let app = servers.app.clone();
                let name = name.clone();
                async move { app.plugin.remove_plugin(&name, PluginType::Mcp).await }
            })
            .await;
            match result {
                Ok(()) => refresh(servers),
                Err(e) => show_error(&servers.window, &e.to_string()),
            }
        });
    });
    button
}

/// Register the submitted config — replacing the same-named plugin when
/// `editing`. The outcome goes to `done`: the dialog closes on success
/// and shows the failure in its banner otherwise.
fn submit(servers: McpSection, editing: bool, config: Plugin, done: SubmitDone) {
    glib::MainContext::default().spawn_local(async move {
        let result = runtime::spawn({
            let app = servers.app.clone();
            async move {
                if editing {
                    app.plugin.update_plugin(PluginType::Mcp, config).await
                } else {
                    app.plugin.add_mcp(config).await
                }
            }
        })
        .await;
        match result {
            Ok(()) => {
                refresh(servers);
                done(Ok(()));
            },
            Err(e) => done(Err(e.to_string())),
        }
    });
}

fn show_error(window: &Window, message: &str) {
    AlertDialog::builder()
        .modal(true)
        .message("Plugin operation failed")
        .detail(message)
        .build()
        .show(Some(window));
}
