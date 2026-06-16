// Add-plugin dialog: a small form for registering an MCP server, either a
// local command or a remote HTTP endpoint.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gtk4::{Box as GtkBox, Button, ListBox, Orientation, SelectionMode, StringList, glib};
use libadwaita::{
    Banner, ComboRow, Dialog, EntryRow, ExpanderRow, SpinRow, SwitchRow, ToolbarView, prelude::*,
};
use scry_core::{Plugin, PluginArgs, Transport};

/// Default plugin timeout, in seconds. Matches the storage default.
const DEFAULT_TIMEOUT: i64 = 300;

/// Delay between the last keystroke and a validation pass, so fields
/// aren't flagged red mid-typing.
const VALIDATE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Outcome callback handed to `on_submit`: report `Ok` to close the
/// dialog, `Err(message)` to keep it open with the message in its banner.
pub(super) type SubmitDone = Rc<dyn Fn(Result<(), String>)>;

/// Open the dialog. `taken` is the set of existing plugin names (new names
/// must be unique; when editing, pass the set without the edited plugin's
/// own name). `initial` pre-fills the form for editing; its name and
/// disabled state carry over unchanged. `on_submit` fires with the
/// completed config when the user confirms and must eventually call the
/// provided [`SubmitDone`]; closing the dialog any other way submits
/// nothing.
pub(super) fn open(
    parent: &impl IsA<gtk4::Widget>,
    taken: HashSet<String>,
    initial: Option<Plugin>,
    on_submit: Rc<dyn Fn(Plugin, SubmitDone)>,
) {
    let editing = initial.is_some();
    // Not edited by the form; carried over from the edited plugin.
    let disabled = initial.as_ref().is_some_and(|p| p.disabled);

    let dialog = Dialog::builder()
        .title(if editing { "Edit Plugin" } else { "Add Plugin" })
        .content_width(440)
        .build();

    let cancel = Button::with_label("Cancel");
    let add = Button::builder()
        .label(if editing { "Save" } else { "Add" })
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();

    let header = libadwaita::HeaderBar::builder()
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

    let env = EntryRow::builder()
        .title("Environment variables (JSON)")
        .build();
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

    // Submit failures land here; the dialog stays open for corrections.
    let banner = Banner::new("");
    banner.set_button_label(Some("Dismiss"));
    banner.connect_button_clicked(|banner| banner.set_revealed(false));

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.append(&banner);
    body.append(&form);

    let view = ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&body));
    dialog.set_child(Some(&view));

    // Everything outside Advanced is required. Required-but-empty fields
    // only keep Add disabled; fields with *invalid* content additionally
    // get Adwaita's red error outline and a tooltip saying why.
    let validate: Rc<dyn Fn() -> bool> = {
        let add = add.clone();
        let name = name.clone();
        let kind = kind.clone();
        let command = command.clone();
        let args = args.clone();
        let url = url.clone();
        let env = env.clone();
        Rc::new(move || {
            let filled = |row: &EntryRow| !row.text().trim().is_empty();

            // Name: required and unique among existing plugins.
            let name_text = name.text();
            let duplicate = taken.contains(name_text.trim());
            flag(&name, duplicate, "A plugin with this name already exists.");
            let name_ok = filled(&name) && !duplicate;

            // Source: command + arguments for local, a valid URL for remote.
            let source_ok = if kind.selected() == 0 {
                flag(&url, false, "");
                filled(&command) && filled(&args)
            } else {
                let url_text = url.text();
                let url_text = url_text.trim();
                let invalid = !url_text.is_empty() && !is_valid_url(url_text);
                flag(&url, invalid, "Must be a valid http(s) URL.");
                !url_text.is_empty() && !invalid
            };

            // Env: optional, but must be a JSON object of string values.
            let env_text = env.text();
            let env_text = env_text.trim();
            let env_invalid = !env_text.is_empty()
                && serde_json::from_str::<HashMap<String, String>>(env_text).is_err();
            flag(
                &env,
                env_invalid,
                r#"Must be a JSON object like {"KEY": "value"}."#,
            );

            let ok = name_ok && source_ok && !env_invalid;
            add.set_sensitive(ok);
            ok
        })
    };

    // Debounced re-validation for text fields: disable Add immediately,
    // validate once typing pauses. The Add handler re-validates
    // synchronously, so the debounce window can't let a stale-enabled
    // button submit an invalid form.
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let queue_validate: Rc<dyn Fn()> = {
        let validate = validate.clone();
        let pending = pending.clone();
        let add = add.clone();
        Rc::new(move || {
            add.set_sensitive(false);
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
            let validate = validate.clone();
            let pending_done = pending.clone();
            *pending.borrow_mut() =
                Some(glib::timeout_add_local_once(VALIDATE_DEBOUNCE, move || {
                    // The source is gone once it fires; forget its id so a
                    // later keystroke doesn't try to remove it again.
                    pending_done.borrow_mut().take();
                    validate();
                }));
        })
    };
    for row in [&name, &command, &args, &url, &env] {
        let queue_validate = queue_validate.clone();
        row.connect_changed(move |_| queue_validate());
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
            // No typing burst to absorb on a combo; validate right away.
            validate();
        });
    }

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    // Pre-fill after the handlers are wired, so the kind switch updates
    // the visible rows and validation arms the confirm button.
    if let Some(initial) = initial {
        name.set_text(&initial.name);
        // Names are immutable; updates address the plugin by name.
        name.set_editable(false);
        timeout.set_value(initial.timeout as f64);
        if !initial.env.is_empty() {
            env.set_text(&serde_json::to_string(&initial.env).unwrap_or_default());
        }
        match &initial.args {
            PluginArgs::Local {
                command: cmd,
                args: arg_list,
            } => {
                kind.set_selected(0);
                command.set_text(cmd);
                args.set_text(&arg_list.join(" "));
            },
            PluginArgs::Remote {
                url: address,
                requires_auth: auth,
            } => {
                kind.set_selected(1);
                url.set_text(address);
                requires_auth.set_active(*auth);
            },
        }
        validate();
    }

    {
        let dialog = dialog.clone();
        let name = name.clone();
        let validate = validate.clone();
        add.connect_clicked(move |button| {
            if !validate() {
                return;
            }
            // Lock the button while the submission is in flight; it comes
            // back (with the banner) only if the submission fails.
            banner.set_revealed(false);
            button.set_sensitive(false);

            // Validation just confirmed the env text parses.
            let env_text = env.text();
            let env_text = env_text.trim();
            let env_map: HashMap<String, String> = if env_text.is_empty() {
                HashMap::new()
            } else {
                serde_json::from_str(env_text).unwrap_or_default()
            };

            let (transport, plugin_args) = if kind.selected() == 0 {
                (
                    Transport::Local,
                    PluginArgs::Local {
                        command: command.text().trim().to_string(),
                        args: args.text().split_whitespace().map(str::to_string).collect(),
                    },
                )
            } else {
                (
                    Transport::Http,
                    PluginArgs::Remote {
                        url: url.text().trim().to_string(),
                        requires_auth: requires_auth.is_active(),
                    },
                )
            };

            let done: SubmitDone = {
                let dialog = dialog.clone();
                let banner = banner.clone();
                let button = button.clone();
                Rc::new(move |result| match result {
                    Ok(()) => {
                        dialog.close();
                    },
                    Err(message) => {
                        banner.set_title(&message);
                        banner.set_revealed(true);
                        button.set_sensitive(true);
                    },
                })
            };
            on_submit(
                Plugin {
                    name: name.text().trim().to_string(),
                    transport,
                    timeout: timeout.value() as i64,
                    disabled,
                    env: env_map,
                    args: plugin_args,
                },
                done,
            );
        });
    }

    dialog.present(Some(parent));
}

/// Toggle Adwaita's red error outline and an explanatory tooltip.
fn flag(row: &impl IsA<gtk4::Widget>, error: bool, reason: &str) {
    if error {
        row.add_css_class("error");
        row.set_tooltip_text(Some(reason));
    } else {
        row.remove_css_class("error");
        row.set_tooltip_text(None);
    }
}

/// True for an absolute http(s) URL with a non-empty host.
fn is_valid_url(text: &str) -> bool {
    match glib::Uri::parse(text, glib::UriFlags::NONE) {
        Ok(uri) => {
            matches!(uri.scheme().as_str(), "http" | "https")
                && uri.host().is_some_and(|host| !host.is_empty())
        },
        Err(_) => false,
    }
}
