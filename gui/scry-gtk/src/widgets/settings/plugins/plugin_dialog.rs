use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use futures::channel::mpsc;
use gtk4::{
    Adjustment, Box as GtkBox, Button, ListBox, Orientation, SelectionMode, StringList, glib,
    prelude::*,
};
use libadwaita::{
    Banner, ComboRow, Dialog, EntryRow, ExpanderRow, HeaderBar, SpinRow, SwitchRow, ToolbarView,
    prelude::*,
};
use scry_core::{Plugin, PluginArgs, Transport};

use crate::widgets::settings::plugins::model::Msg;

#[derive(Default, PartialEq)]
enum Kind {
    #[default]
    Local,
    Remote,
}

pub(super) struct PluginDialog {
    dialog: Dialog,
    banner: Banner,
    form: ListBox,
    add: Button,
    cancel: Button,
}

impl PluginDialog {
    pub(super) fn new(
        plugin: Option<Plugin>,
        taken: HashSet<String>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let initial = plugin
            .as_ref()
            .map(FormData::from_plugin)
            .unwrap_or_default();
        let editing = plugin.is_some();
        let disabled = plugin.as_ref().is_some_and(|p| p.disabled);
        let remote = initial.kind == Kind::Remote;

        // [name, command, args / url, env]
        let status = Rc::new(Cell::new(if editing {
            [true; 4]
        } else {
            [false, false, false, true]
        }));

        let cancel = Button::builder().label("Cancel").build();
        {
            let dispatcher = dispatcher.clone();
            cancel.connect_clicked(move |_| {
                let _ = dispatcher.unbounded_send(Msg::McpDialogCancelled);
            });
        }
        let add = Button::builder()
            .label(if editing { "Save" } else { "Add" })
            .sensitive(editing)
            .css_classes(["suggested-action"])
            .build();
        let update_status = {
            let status = status.clone();
            let add = add.clone();
            move |index: usize, ok: bool| {
                let mut slots = status.get();
                slots[index] = ok;
                status.set(slots);
                add.set_sensitive(slots == [true; 4]);
            }
        };

        let header = HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&add);

        let banner = Banner::builder().button_label("Dismiss").build();
        banner.connect_button_clicked(|banner| banner.set_revealed(false));

        let name = EntryRow::builder()
            .title("Name")
            .text(&initial.name)
            .editable(!editing)
            .build();
        validate(&name, 0, true, update_status.clone(), move |text| {
            validate_name(text, &taken)
        });
        let kind = ComboRow::builder()
            .title("Type")
            .model(&StringList::new(&["Local command", "Remote server"]))
            .selected(remote as u32)
            .build();
        let command = EntryRow::builder()
            .title("Command")
            .text(&initial.command)
            .visible(!remote)
            .build();
        let args = EntryRow::builder()
            .title("Arguments (JSON array)")
            .tooltip_text(r#"e.g. ["--port", "8080"]"#)
            .text(&initial.args)
            .visible(!remote)
            .build();
        validate(&command, 1, true, update_status.clone(), |_| None);
        validate(&args, 2, true, update_status.clone(), validate_args);
        let url = EntryRow::builder()
            .title("URL")
            .text(&initial.url)
            .visible(remote)
            .build();
        validate(&url, 2, true, update_status.clone(), validate_url);
        let requires_auth = SwitchRow::builder()
            .title("Requires authentication")
            .active(initial.requires_auth)
            .visible(remote)
            .build();
        // Switching the kind swaps which rows are shown, so the slots of the
        // swapped rows are re-judged: hidden rows are forced valid, shown
        // ones take their text's verdict.
        {
            let update_status = update_status.clone();
            let command = command.clone();
            let args = args.clone();
            let url = url.clone();
            let requires_auth = requires_auth.clone();
            kind.connect_selected_notify(move |kind| {
                let remote = kind.selected() == 1;
                command.set_visible(!remote);
                args.set_visible(!remote);
                url.set_visible(remote);
                requires_auth.set_visible(remote);
                let source = if remote { url.text() } else { args.text() };
                let error = if remote {
                    validate_url(&source)
                } else {
                    validate_args(&source)
                };
                update_status(1, remote || !command.text().trim().is_empty());
                update_status(2, !source.trim().is_empty() && error.is_none());
            });
        }

        let timeout = SpinRow::builder()
            .title("Timeout")
            .subtitle("Seconds. Defaults to 300.")
            .adjustment(&Adjustment::new(
                initial.timeout as f64,
                1.0,
                3600.0,
                10.0,
                0.0,
                0.0,
            ))
            .build();
        let env = EntryRow::builder()
            .title("Environment variables (JSON)")
            .tooltip_text(r#"e.g. {"API_KEY": "secret"}"#)
            .text(&initial.env)
            .build();
        validate(&env, 3, false, update_status, validate_env);
        let advanced = ExpanderRow::builder()
            .title("Advanced")
            .subtitle("Optional")
            .build();
        advanced.add_row(&timeout);
        advanced.add_row(&env);

        let form = ListBox::builder()
            .selection_mode(SelectionMode::None)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .css_classes(["boxed-list"])
            .build();
        form.append(&name);
        form.append(&kind);
        form.append(&command);
        form.append(&args);
        form.append(&url);
        form.append(&requires_auth);
        form.append(&advanced);

        let content = GtkBox::new(Orientation::Vertical, 0);
        content.append(&banner);
        content.append(&form);

        let view = ToolbarView::builder().content(&content).build();
        view.add_top_bar(&header);

        let dialog = Dialog::builder()
            .title(if editing { "Edit Plugin" } else { "Add Plugin" })
            .content_width(440)
            .child(&view)
            .build();

        // Submit reads the widgets at click time and reports through the
        // page's event loop. The form locks until the verdict arrives:
        // `show_error` unlocks on failure, success closes the dialog.
        {
            let dispatcher = dispatcher.clone();
            let banner = banner.clone();
            let form = form.clone();
            let cancel = cancel.clone();
            // Weak: the closure lives on a child of the dialog.
            let dialog = dialog.downgrade();
            let name = name.clone();
            let kind = kind.clone();
            let command = command.clone();
            let args = args.clone();
            let url = url.clone();
            let requires_auth = requires_auth.clone();
            let timeout = timeout.clone();
            let env = env.clone();
            add.connect_clicked(move |add| {
                add.set_sensitive(false);
                cancel.set_sensitive(false);
                form.set_sensitive(false);
                banner.set_revealed(false);
                if let Some(dialog) = dialog.upgrade() {
                    dialog.set_can_close(false);
                }
                let data = FormData {
                    name: name.text().into(),
                    kind: if kind.selected() == 1 {
                        Kind::Remote
                    } else {
                        Kind::Local
                    },
                    command: command.text().into(),
                    args: args.text().into(),
                    url: url.text().into(),
                    requires_auth: requires_auth.is_active(),
                    timeout: timeout.value() as u32,
                    env: env.text().into(),
                };
                let _ = dispatcher.unbounded_send(Msg::McpDialogSubmitted {
                    config: data.to_plugin(disabled),
                    editing,
                });
            });
        }
        dialog.connect_closed(move |_| {
            let _ = dispatcher.unbounded_send(Msg::McpDialogCancelled);
        });

        Self {
            dialog,
            banner,
            form,
            add,
            cancel,
        }
    }

    pub(super) fn show(&self, parent: &impl IsA<gtk4::Widget>) {
        self.dialog.present(Some(parent));
    }

    pub(super) fn hide(&self) {
        self.dialog.force_close();
    }

    pub(super) fn show_error(&self, error_msg: &str) {
        self.banner.set_title(error_msg);
        self.banner.set_revealed(true);
        self.form.set_sensitive(true);
        self.cancel.set_sensitive(true);
        self.add.set_sensitive(true);
        self.dialog.set_can_close(true);
    }
}

fn validate(
    row: &EntryRow,
    index: usize,
    required: bool,
    update_status: impl Fn(usize, bool) + 'static,
    validate: impl Fn(&str) -> Option<&'static str> + 'static,
) {
    row.connect_changed(move |row| {
        let text = row.text();
        let error = validate(&text);
        flag(row, error);
        // An empty required field is not flagged red, but it is not
        // submittable either.
        let ok = error.is_none() && (!required || !text.trim().is_empty());
        update_status(index, ok);
    });
}

fn validate_name(name: &str, taken: &HashSet<String>) -> Option<&'static str> {
    taken
        .contains(name.trim())
        .then_some("A plugin with this name already exists.")
}

fn validate_args(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.is_empty() {
        // Unflagged; the required rule already withholds submit.
        return None;
    }
    match parse_args(text) {
        Ok(args) if !args.is_empty() => None,
        _ => Some(r#"Must be a non-empty JSON array like ["--flag", "value"]."#),
    }
}

fn validate_url(url: &str) -> Option<&'static str> {
    let url = url.trim();
    (!url.is_empty() && !is_valid_url(url)).then_some("Must be a valid http(s) URL.")
}

fn validate_env(text: &str) -> Option<&'static str> {
    parse_env(text)
        .is_err()
        .then_some(r#"Must be a JSON object like {"KEY": "value"}."#)
}

/// Toggle Adwaita's red error outline and an explanatory tooltip.
fn flag(row: &impl IsA<gtk4::Widget>, error: Option<&str>) {
    match error {
        Some(reason) => {
            row.add_css_class("error");
            row.set_tooltip_text(Some(reason));
        },
        None => {
            row.remove_css_class("error");
            row.set_tooltip_text(None);
        },
    }
}

struct FormData {
    name: String,
    kind: Kind,
    command: String,
    args: String,
    url: String,
    requires_auth: bool,
    timeout: u32,
    env: String,
}

impl Default for FormData {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: Kind::default(),
            command: String::new(),
            args: String::new(),
            url: String::new(),
            requires_auth: false,
            timeout: 300,
            env: String::new(),
        }
    }
}

impl FormData {
    fn from_plugin(initial: &Plugin) -> Self {
        let mut form = Self {
            name: initial.name.clone(),
            timeout: initial.timeout,
            ..Self::default()
        };

        if !initial.env.is_empty() {
            form.env = serde_json::to_string(&initial.env).unwrap_or_default();
        }
        match &initial.args {
            PluginArgs::Local { command, args } => {
                form.kind = Kind::Local;
                form.command = command.clone();
                if !args.is_empty() {
                    form.args = serde_json::to_string(args).unwrap_or_default();
                }
            },
            PluginArgs::Remote { url, requires_auth } => {
                form.kind = Kind::Remote;
                form.url = url.clone();
                form.requires_auth = *requires_auth;
            },
        }

        form
    }

    fn to_plugin(&self, disabled: bool) -> Plugin {
        let (transport, args) = match self.kind {
            Kind::Local => (
                Transport::Local,
                PluginArgs::Local {
                    command: self.command.trim().to_string(),
                    args: parse_args(&self.args).unwrap_or_default(),
                },
            ),
            Kind::Remote => (
                Transport::Http,
                PluginArgs::Remote {
                    url: self.url.trim().to_string(),
                    requires_auth: self.requires_auth,
                },
            ),
        };
        Plugin {
            name: self.name.trim().to_string(),
            transport,
            timeout: self.timeout,
            disabled,
            env: parse_env(&self.env).unwrap_or_default(),
            args,
        }
    }
}

fn parse_args(text: &str) -> Result<Vec<String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(Vec::new())
    } else {
        serde_json::from_str(text)
    }
}

fn parse_env(text: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(HashMap::new())
    } else {
        serde_json::from_str(text)
    }
}

fn is_valid_url(text: &str) -> bool {
    match glib::Uri::parse(text, glib::UriFlags::NONE) {
        Ok(uri) => {
            matches!(uri.scheme().as_str(), "http" | "https")
                && uri.host().is_some_and(|host| !host.is_empty())
        },
        Err(_) => false,
    }
}
