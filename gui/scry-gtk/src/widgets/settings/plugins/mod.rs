// Plugins tab — native plugins (none yet) and user-managed MCP servers.

mod modal;

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use gtk4::{Align, Box as GtkBox, Button, Orientation, Switch, glib};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, ButtonRow, ExpanderRow, PreferencesGroup,
    PreferencesPage, prelude::*,
};
use modal::SubmitDone;
use scry_core::{AppContext, HealthStatus, McpServer, Plugin, PluginArgs, PluginType};

use super::Group;
use crate::runtime;

pub(super) fn build(app: Arc<AppContext>, window: ApplicationWindow) -> PreferencesPage {
    // Both groups end in an add row, which doubles as the empty state.
    let plugins = PreferencesGroup::builder().title("Plugins").build();
    plugins.add(
        &ButtonRow::builder()
            .title("Add Plugin…")
            .start_icon_name("list-add-symbolic")
            .sensitive(false)
            .tooltip_text("Native plugins are not supported yet.")
            .build(),
    );

    let mcp = Group::new("MCP Servers");
    let add = ButtonRow::builder()
        .title("Add MCP Server…")
        .start_icon_name("list-add-symbolic")
        .build();
    mcp.add(add.clone());

    let servers = McpSection {
        group: mcp.clone(),
        add: add.clone(),
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
            modal::open(&servers.window, taken, None, on_submit);
        });
    }

    let page = PreferencesPage::new();
    page.add(&plugins);
    page.add(&mcp.widget);
    page
}

#[derive(Clone)]
struct McpSection {
    group: Group,
    add: ButtonRow,
    /// Names of the listed servers; the add dialog rejects duplicates.
    names: Rc<RefCell<HashSet<String>>>,
    app: Arc<AppContext>,
    window: ApplicationWindow,
}

fn refresh(servers: McpSection) {
    let app = servers.app.clone();
    runtime::spawn_with(
        async move { app.plugin.list_mcps().await },
        move |result| match result {
            Ok(list) => {
                *servers.names.borrow_mut() = list.iter().map(|s| s.config.name.clone()).collect();
                servers.group.clear();
                for server in list {
                    servers.group.add(mcp_row(&servers, &server));
                }
                // The add row sits last so new servers appear above it.
                servers.group.add(servers.add.clone());
            },
            Err(e) => log::warn!("list_mcps failed: {e}"),
        },
    );
}

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
        HealthStatus::Unhealthy => actions.append(&super::unhealthy_icon(server.error.as_deref())),
    }
    actions.append(&edit_button(servers, config));
    actions.append(&remove_button(servers, config));
    row.add_suffix(&actions);
    row
}

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
        modal::open(&servers.window, taken, Some(config.clone()), on_submit);
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
        let app = servers.app.clone();
        let name = name.clone();
        runtime::spawn_with(
            async move { app.plugin.toggle_plugin(&name, !state).await },
            move |result| {
                if let Err(e) = result {
                    show_error(&servers.window, &e.to_string());
                }
            },
        );
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
        let app = servers.app.clone();
        let name = name.clone();
        runtime::spawn_with(
            async move { app.plugin.remove_plugin(&name, PluginType::Mcp).await },
            move |result| match result {
                Ok(()) => refresh(servers),
                Err(e) => show_error(&servers.window, &e.to_string()),
            },
        );
    });
    button
}

fn submit(servers: McpSection, editing: bool, config: Plugin, done: SubmitDone) {
    let app = servers.app.clone();
    runtime::spawn_with(
        async move {
            if editing {
                app.plugin.update_plugin(PluginType::Mcp, config).await
            } else {
                app.plugin.add_mcp(config).await
            }
        },
        move |result| match result {
            Ok(()) => {
                refresh(servers);
                done(Ok(()));
            },
            Err(e) => done(Err(e.to_string())),
        },
    );
}

fn show_error(window: &ApplicationWindow, message: &str) {
    let dialog = AlertDialog::builder()
        .heading("Plugin Operation Failed")
        .body(message)
        .close_response("close")
        .build();
    dialog.add_response("close", "Close");
    dialog.present(Some(window));
}
