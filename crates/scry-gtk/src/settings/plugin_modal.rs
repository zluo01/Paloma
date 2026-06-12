// Add-plugin dialog: a small form for registering an MCP server, either a
// local command or a remote HTTP endpoint.

use std::rc::Rc;

use adw::{prelude::*, ComboRow, Dialog, EntryRow, ExpanderRow, SpinRow, SwitchRow, ToolbarView};
use gtk4::{Button, ListBox, SelectionMode, StringList};
use libadwaita as adw;

/// Default plugin timeout, in seconds. Matches the storage default.
pub const DEFAULT_TIMEOUT: i64 = 300;

/// Values collected by the form. Mirrors `scry_storage`'s plugin shape.
#[derive(Debug)]
pub struct NewPlugin {
    pub name: String,
    pub source: PluginSource,
    /// Seconds.
    pub timeout: i64,
    /// Raw JSON object of environment variables; empty = none.
    pub env: String,
}

#[derive(Debug)]
pub enum PluginSource {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

/// Open the dialog. `on_submit` fires with the completed form when the
/// user clicks Add; closing the dialog any other way submits nothing.
pub fn open(parent: &impl IsA<gtk4::Widget>, on_submit: Rc<dyn Fn(NewPlugin)>) {
    let dialog = Dialog::builder()
        .title("Add Plugin")
        .content_width(440)
        .build();

    let cancel = Button::with_label("Cancel");
    let add = Button::builder()
        .label("Add")
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();

    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&add);

    // One continuous group; the Type row hides/shows the rows that only
    // apply to its selection. Advanced (optional) fields sit in an
    // expander at the end.
    let name = EntryRow::builder().title("Name").build();
    let kind = ComboRow::builder()
        .title("Type")
        .model(&StringList::new(&["Local command", "Remote server"]))
        .build();
    let command = EntryRow::builder().title("Command").build();
    let args = EntryRow::builder().title("Arguments").build();
    let url = EntryRow::builder().title("URL").visible(false).build();
    let requires_auth = SwitchRow::builder()
        .title("Requires authentication")
        .visible(false)
        .build();

    let timeout = SpinRow::with_range(1.0, 3600.0, 10.0);
    timeout.set_title("Timeout");
    timeout.set_subtitle("Seconds. Defaults to 300.");
    timeout.set_value(DEFAULT_TIMEOUT as f64);

    let env = EntryRow::builder().title("Environment variables (JSON)").build();
    env.set_tooltip_text(Some(r#"e.g. {"API_KEY": "secret"}"#));

    let advanced = ExpanderRow::builder()
        .title("Advanced")
        .subtitle("Optional")
        .build();
    advanced.add_row(&timeout);
    advanced.add_row(&env);

    let form = ListBox::builder()
        .selection_mode(SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    for row in [
        name.clone().upcast::<gtk4::Widget>(),
        kind.clone().upcast(),
        command.clone().upcast(),
        args.clone().upcast(),
        url.clone().upcast(),
        requires_auth.clone().upcast(),
        advanced.upcast(),
    ] {
        form.append(&row);
    }

    let view = ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&form));
    dialog.set_child(Some(&view));

    // Everything outside Advanced is required; Add stays disabled until
    // the visible required rows are filled in.
    let validate: Rc<dyn Fn()> = {
        let add = add.clone();
        let name = name.clone();
        let kind = kind.clone();
        let command = command.clone();
        let args = args.clone();
        let url = url.clone();
        Rc::new(move || {
            let filled = |row: &EntryRow| !row.text().trim().is_empty();
            let source_ok = if kind.selected() == 0 {
                filled(&command) && filled(&args)
            } else {
                filled(&url)
            };
            add.set_sensitive(filled(&name) && source_ok);
        })
    };
    for row in [&name, &command, &args, &url] {
        let validate = validate.clone();
        row.connect_changed(move |_| validate());
    }

    {
        let command = command.clone();
        let args = args.clone();
        let url = url.clone();
        let requires_auth = requires_auth.clone();
        let validate = validate.clone();
        kind.connect_selected_notify(move |row| {
            let local = row.selected() == 0;
            command.set_visible(local);
            args.set_visible(local);
            url.set_visible(!local);
            requires_auth.set_visible(!local);
            validate();
        });
    }

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    {
        let dialog = dialog.clone();
        add.connect_clicked(move |_| {
            let source = if kind.selected() == 0 {
                PluginSource::Local {
                    command: command.text().trim().to_string(),
                    args: args.text().split_whitespace().map(str::to_string).collect(),
                }
            } else {
                PluginSource::Remote {
                    url: url.text().trim().to_string(),
                    requires_auth: requires_auth.is_active(),
                }
            };
            on_submit(NewPlugin {
                name: name.text().trim().to_string(),
                source,
                timeout: timeout.value() as i64,
                env: env.text().trim().to_string(),
            });
            dialog.close();
        });
    }

    dialog.present(Some(parent));
}
