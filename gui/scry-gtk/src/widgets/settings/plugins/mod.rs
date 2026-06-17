//! Plugins settings page.
//!
//! Native plugins are disabled. MCP servers are loaded from the backend,
//! rendered as expandable rows, and edited through [`modal`].

mod modal;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

use gtk4::{Align, Box as GtkBox, Button, Orientation, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, ButtonRow, ExpanderRow, PreferencesGroup,
    PreferencesPage, prelude::*,
};
use modal::{OnSubmit, SubmitDone};
use scry_core::{AppContext, HealthStatus, McpServer, Plugin, PluginArgs, PluginType};

use super::Group;
use crate::runtime;

/// MCP plugins page controller.
pub(super) struct PluginsPage {
    page: PreferencesPage,
    /// Configured MCP servers, rebuilt on refresh, plus the persistent add row.
    mcp: Group,
    /// "Add MCP Server" row, reattached after each refresh so it stays last.
    add: ButtonRow,
    /// Names from the last successful refresh; dialogs reject duplicates.
    names: RefCell<HashSet<String>>,
    app_context: Arc<AppContext>,
    /// Dialog parent. Weak because the window owns this page's widget tree.
    window: glib::WeakRef<ApplicationWindow>,
}

pub(super) fn build(app_context: Arc<AppContext>, window: ApplicationWindow) -> Rc<PluginsPage> {
    PluginsPage::new(app_context, &window)
}

impl PluginsPage {
    fn new(app_context: Arc<AppContext>, window: &ApplicationWindow) -> Rc<Self> {
        let page = PreferencesPage::new();

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

        let mcp = Group::new("MCP Servers");
        let add = ButtonRow::builder()
            .title("Add MCP Server…")
            .start_icon_name("list-add-symbolic")
            .build();
        mcp.add(add.clone());

        page.add(&plugins);
        page.add(&mcp.widget);

        let this = Rc::new(Self {
            page,
            mcp,
            add,
            names: RefCell::new(HashSet::new()),
            app_context,
            window: window.downgrade(),
        });
        this.wire_add();
        this.refresh();
        this
    }

    pub(super) fn widget(&self) -> &PreferencesPage {
        &self.page
    }

    fn window(&self) -> ApplicationWindow {
        self.window
            .upgrade()
            .expect("settings window outlives the page")
    }

    fn wire_add(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.add.connect_activated(move |_| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let submit_weak = Rc::downgrade(&this);
            let on_submit: OnSubmit = Rc::new(move |config, done| {
                if let Some(this) = submit_weak.upgrade() {
                    this.submit(false, config, done);
                }
            });
            let taken = this.names.borrow().clone();
            modal::open(&this.window(), taken, None, on_submit);
        });
    }

    fn refresh(self: &Rc<Self>) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.plugin.list_mcps().await },
            move |result| match result {
                Ok(list) => {
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    *this.names.borrow_mut() = list.iter().map(|s| s.config.name.clone()).collect();
                    this.mcp.clear();
                    for server in &list {
                        this.mcp.add(this.mcp_row(server));
                    }
                    // The add row sits last so new servers appear above it.
                    this.mcp.add(this.add.clone());
                },
                Err(e) => log::warn!("list_mcps failed: {e}"),
            },
        );
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
            HealthStatus::Running => actions.append(&self.toggle(config)),
            HealthStatus::Unhealthy => {
                actions.append(&super::unhealthy_icon(server.error.as_deref()))
            },
        }
        actions.append(&self.edit_button(config));
        actions.append(&self.remove_button(config));
        row.add_suffix(&actions);
        row
    }

    /// Open the edit dialog. The server keeps its existing name, so that name
    /// is removed from the duplicate-name set passed to the dialog.
    fn edit_button(self: &Rc<Self>, config: &Plugin) -> Button {
        let button = Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Edit plugin")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let weak = Rc::downgrade(self);
        let config = config.clone();
        button.connect_clicked(move |_| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let submit_weak = Rc::downgrade(&this);
            let on_submit: OnSubmit = Rc::new(move |config, done| {
                if let Some(this) = submit_weak.upgrade() {
                    this.submit(true, config, done);
                }
            });
            let mut taken = this.names.borrow().clone();
            taken.remove(&config.name);
            modal::open(&this.window(), taken, Some(config.clone()), on_submit);
        });
        button
    }

    /// Toggle the stored disabled flag. The switch state is committed only after
    /// the backend confirms the save.
    fn toggle(self: &Rc<Self>, config: &Plugin) -> Switch {
        let switch = Switch::builder()
            .active(!config.disabled)
            .valign(Align::Center)
            .tooltip_text("Enable or disable the plugin")
            .build();

        let name = config.name.clone();
        // Set while reverting the slider after a failed save, so the revert does
        // not re-enter this handler and issue another save.
        let reverting = Rc::new(Cell::new(false));
        let weak = Rc::downgrade(self);
        // Return `Stop` so GTK does not commit `state` before the backend save
        // completes. On success we commit with `set_state`; on failure we move
        // the slider back with `set_active`.
        switch.connect_state_set(move |sw, state| {
            if reverting.get() {
                return glib::Propagation::Proceed;
            }
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let sw = sw.clone();
            let app_context = this.app_context.clone();
            let name = name.clone();
            let reverting = reverting.clone();
            let error_weak = Rc::downgrade(&this);
            runtime::spawn_with(
                async move { app_context.plugin.toggle_plugin(&name, !state).await },
                move |result| match result {
                    Ok(()) => sw.set_state(state),
                    Err(e) => {
                        reverting.set(true);
                        sw.set_active(!state);
                        reverting.set(false);
                        if let Some(this) = error_weak.upgrade() {
                            show_error(&this.window(), &e.to_string());
                        }
                    },
                },
            );
            glib::Propagation::Stop
        });
        switch
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
            let Some(this) = weak.upgrade() else {
                return;
            };
            let app_context = this.app_context.clone();
            let name = name.clone();
            let done_weak = Rc::downgrade(&this);
            runtime::spawn_with(
                async move {
                    app_context
                        .plugin
                        .remove_plugin(&name, PluginType::Mcp)
                        .await
                },
                move |result| {
                    let Some(this) = done_weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(()) => this.refresh(),
                        Err(e) => show_error(&this.window(), &e.to_string()),
                    }
                },
            );
        });
        button
    }

    /// Persist a dialog result and report the outcome back to the dialog.
    fn submit(self: &Rc<Self>, editing: bool, config: Plugin, done: SubmitDone) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move {
                if editing {
                    app_context
                        .plugin
                        .update_plugin(PluginType::Mcp, config)
                        .await
                } else {
                    app_context.plugin.add_mcp(config).await
                }
            },
            move |result| match result {
                Ok(()) => {
                    if let Some(this) = weak.upgrade() {
                        this.refresh();
                    }
                    done(Ok(()));
                },
                Err(e) => done(Err(e.to_string())),
            },
        );
    }
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

fn show_error(window: &ApplicationWindow, message: &str) {
    let dialog = AlertDialog::builder()
        .heading("Plugin Operation Failed")
        .body(message)
        .close_response("close")
        .build();
    dialog.add_response("close", "Close");
    dialog.present(Some(window));
}
