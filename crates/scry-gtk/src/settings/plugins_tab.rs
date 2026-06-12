// Plugins tab — user-managed MCP plugins. UI shape only for now: added
// plugins land in the list but are not yet wired to the controller.

use std::rc::Rc;

use adw::{prelude::*, ActionRow, ButtonRow};
use gtk4::{Align, Box as GtkBox, Label, ListBox, Orientation, SelectionMode, Widget, Window};
use libadwaita as adw;

use super::plugin_modal::{self, NewPlugin, PluginSource};

/// Build the Plugins tab; `window` parents the add-plugin dialog.
pub fn build(window: Window) -> Widget {
    // Trailing "Add Plugin" row; plugin rows are inserted above it.
    let add = ButtonRow::builder()
        .title("Add Plugin…")
        .start_icon_name("list-add-symbolic")
        .build();

    let list = ListBox::builder()
        .selection_mode(SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&add);

    {
        let list = list.clone();
        add.connect_activated(move |add| {
            let on_submit: Rc<dyn Fn(NewPlugin)> = {
                let list = list.clone();
                let add = add.clone();
                Rc::new(move |plugin| {
                    // TODO: register the plugin with the controller.
                    log::info!("add plugin (not yet wired): {plugin:?}");
                    list.insert(&row(&plugin), add.index());
                })
            };
            plugin_modal::open(&window, on_submit);
        });
    }

    let page = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .valign(Align::Start)
        .build();
    page.append(
        &Label::builder()
            .label("MCP Servers")
            .halign(Align::Start)
            .css_classes(["heading"])
            .build(),
    );
    page.append(&list);
    page.upcast()
}

fn row(plugin: &NewPlugin) -> ActionRow {
    let mut detail = match &plugin.source {
        PluginSource::Local { command, args } => format!("Local · {command} {}", args.join(" ")),
        PluginSource::Remote {
            url,
            requires_auth: true,
        } => format!("Remote · {url} · requires auth"),
        PluginSource::Remote { url, .. } => format!("Remote · {url}"),
    };
    if plugin.timeout != plugin_modal::DEFAULT_TIMEOUT {
        detail.push_str(&format!(" · timeout {}s", plugin.timeout));
    }
    if !plugin.env.is_empty() {
        detail.push_str(" · env");
    }

    ActionRow::builder()
        .title(&plugin.name)
        .subtitle(detail.trim_end())
        .build()
}
