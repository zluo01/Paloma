//! Add/edit dialog for MCP server plugins.
//!
//! The dialog owns validation and form-to-[`Plugin`] conversion. Persistence is
//! delegated through [`OnSubmit`] so the page can decide whether the config is a
//! create or update.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

use gtk4::{glib, prelude::*, subclass::prelude::*};
use libadwaita::{EntryRow, prelude::*};
use scry_core::{Plugin, PluginArgs, Transport};

/// Delay between the last keystroke and a validation pass, so fields
/// aren't flagged red mid-typing.
const VALIDATE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Completion callback passed to [`OnSubmit`].
///
/// `Ok(())` closes the dialog. `Err(message)` keeps it open and displays the
/// message in the banner.
pub(super) type SubmitDone = Rc<dyn Fn(Result<(), String>)>;

/// Form submission hook owned by the page.
///
/// The dialog passes a validated [`Plugin`] and waits for the caller to report
/// persistence success or failure through [`SubmitDone`].
pub(super) type OnSubmit = Rc<dyn Fn(Plugin, SubmitDone)>;

mod imp {
    use std::{
        cell::{Cell, RefCell},
        collections::HashSet,
    };

    use gtk4::{Button, CompositeTemplate, glib, prelude::*, subclass::prelude::*};
    use libadwaita::{
        Banner, ComboRow, EntryRow, SpinRow, SwitchRow, prelude::*, subclass::prelude::*,
    };

    use super::OnSubmit;

    #[derive(CompositeTemplate, Default)]
    #[template(file = "plugin_dialog.ui")]
    pub struct PluginDialog {
        #[template_child]
        pub cancel: TemplateChild<Button>,
        #[template_child]
        pub add: TemplateChild<Button>,
        #[template_child]
        pub banner: TemplateChild<Banner>,
        #[template_child]
        pub name: TemplateChild<EntryRow>,
        #[template_child]
        pub kind: TemplateChild<ComboRow>,
        #[template_child]
        pub command: TemplateChild<EntryRow>,
        #[template_child]
        pub args: TemplateChild<EntryRow>,
        #[template_child]
        pub url: TemplateChild<EntryRow>,
        #[template_child]
        pub requires_auth: TemplateChild<SwitchRow>,
        #[template_child]
        pub timeout: TemplateChild<SpinRow>,
        #[template_child]
        pub env: TemplateChild<EntryRow>,

        /// Names unavailable to this form; validation rejects duplicates.
        pub taken: RefCell<HashSet<String>>,
        /// Preserved from the edited plugin; the dialog does not expose it.
        pub disabled: Cell<bool>,
        pub on_submit: RefCell<Option<OnSubmit>>,
        /// Pending debounced validation pass, cancelled on close or retype.
        pub pending: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PluginDialog {
        const NAME: &'static str = "ScryPluginDialog";
        type Type = super::PluginDialog;
        type ParentType = libadwaita::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PluginDialog {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            self.banner
                .connect_button_clicked(|banner| banner.set_revealed(false));

            self.kind.connect_selected_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.on_kind_changed()
            ));

            for row in [&self.name, &self.command, &self.args, &self.url, &self.env] {
                row.connect_changed(glib::clone!(
                    #[weak]
                    obj,
                    move |_| obj.queue_validate()
                ));
            }

            self.cancel.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));
            self.add.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| obj.on_add_clicked()
            ));
            // Dismissing the dialog cancels pending validation against widgets
            // that are no longer visible.
            obj.connect_closed(glib::clone!(
                #[weak]
                obj,
                move |_| obj.cancel_pending()
            ));
        }
    }

    impl WidgetImpl for PluginDialog {}
    impl AdwDialogImpl for PluginDialog {}
}

glib::wrapper! {
    pub struct PluginDialog(ObjectSubclass<imp::PluginDialog>)
        @extends libadwaita::Dialog, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

/// Open the add/edit dialog.
///
/// `taken` contains names the user may not choose. When editing, omit the
/// edited plugin's own name so keeping it is allowed. `initial` pre-fills edit
/// state. `on_submit` receives the completed config and must eventually call
/// [`SubmitDone`]. Closing the dialog without confirming submits nothing.
pub(super) fn open(
    parent: &impl IsA<gtk4::Widget>,
    taken: HashSet<String>,
    initial: Option<Plugin>,
    on_submit: OnSubmit,
) {
    PluginDialog::new(taken, initial, on_submit).present(Some(parent));
}

impl PluginDialog {
    fn new(taken: HashSet<String>, initial: Option<Plugin>, on_submit: OnSubmit) -> Self {
        let dialog: Self = glib::Object::new();
        let editing = initial.is_some();
        let imp = dialog.imp();
        imp.taken.replace(taken);
        imp.disabled
            .set(initial.as_ref().is_some_and(|p| p.disabled));
        imp.on_submit.replace(Some(on_submit));

        dialog.set_title(if editing { "Edit Plugin" } else { "Add Plugin" });
        imp.add.set_label(if editing { "Save" } else { "Add" });

        // Pre-fill after construction so changing the kind can update visible
        // rows and validation can arm the confirm button.
        if let Some(initial) = initial {
            dialog.prefill(initial);
        }
        dialog
    }

    /// Validate required fields and structured JSON fields.
    ///
    /// Empty required fields keep Add disabled. Invalid fields also get
    /// Adwaita's red error outline and an explanatory tooltip.
    fn validate(&self) -> bool {
        let imp = self.imp();
        let name = imp.name.get();
        let kind = imp.kind.get();
        let command = imp.command.get();
        let args = imp.args.get();
        let url = imp.url.get();
        let env = imp.env.get();

        let filled = |row: &EntryRow| !row.text().trim().is_empty();

        // Name: required and unique among existing plugins.
        let name_text = name.text();
        let duplicate = imp.taken.borrow().contains(name_text.trim());
        flag(&name, duplicate, "A plugin with this name already exists.");
        let name_ok = filled(&name) && !duplicate;

        // Source: command plus JSON arguments for local, valid URL for remote.
        let source_ok = if kind.selected() == 0 {
            flag(&url, false, "");
            // Args are optional, but must be a JSON array of strings so
            // values with spaces keep their boundaries.
            let args_invalid = parse_args(&args.text()).is_err();
            flag(
                &args,
                args_invalid,
                r#"Must be a JSON array like ["--flag", "value"]."#,
            );
            filled(&command) && !args_invalid
        } else {
            flag(&args, false, "");
            let url_text = url.text();
            let url_text = url_text.trim();
            let invalid = !url_text.is_empty() && !is_valid_url(url_text);
            flag(&url, invalid, "Must be a valid http(s) URL.");
            !url_text.is_empty() && !invalid
        };

        // Env: optional, but must be a JSON object of string values.
        let env_invalid = parse_env(&env.text()).is_err();
        flag(
            &env,
            env_invalid,
            r#"Must be a JSON object like {"KEY": "value"}."#,
        );

        let ok = name_ok && source_ok && !env_invalid;
        imp.add.set_sensitive(ok);
        ok
    }

    /// Disable Add immediately and validate after the user stops typing.
    fn queue_validate(&self) {
        let imp = self.imp();
        imp.add.set_sensitive(false);
        if let Some(source) = imp.pending.borrow_mut().take() {
            source.remove();
        }
        let id = glib::timeout_add_local_once(
            VALIDATE_DEBOUNCE,
            glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                move || {
                    // The source is gone once it fires; forget its id so a later
                    // keystroke doesn't try to remove it again.
                    dialog.imp().pending.borrow_mut().take();
                    dialog.validate();
                }
            ),
        );
        imp.pending.replace(Some(id));
    }

    fn cancel_pending(&self) {
        if let Some(source) = self.imp().pending.borrow_mut().take() {
            source.remove();
        }
    }

    fn on_kind_changed(&self) {
        let imp = self.imp();
        let local = imp.kind.selected() == 0;
        imp.command.set_visible(local);
        imp.args.set_visible(local);
        imp.url.set_visible(!local);
        imp.requires_auth.set_visible(!local);
        self.validate();
    }

    fn on_add_clicked(&self) {
        if !self.validate() {
            return;
        }
        let imp = self.imp();
        // Lock the button while the submission is in flight; it comes back
        // (with the banner) only if the submission fails.
        imp.banner.set_revealed(false);
        imp.add.set_sensitive(false);

        // Parsed with the same helpers `validate()` uses; the form is valid here.
        let env_map = parse_env(&imp.env.text()).expect("plugin form validated before submit");

        let (transport, plugin_args) = if imp.kind.selected() == 0 {
            (
                Transport::Local,
                PluginArgs::Local {
                    command: imp.command.text().trim().to_string(),
                    args: parse_args(&imp.args.text())
                        .expect("plugin form validated before submit"),
                },
            )
        } else {
            (
                Transport::Http,
                PluginArgs::Remote {
                    url: imp.url.text().trim().to_string(),
                    requires_auth: imp.requires_auth.is_active(),
                },
            )
        };

        // Weak capture lets the outcome callback no-op if the dialog closes
        // while submission is still in flight.
        let done: SubmitDone = Rc::new(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |result| match result {
                Ok(()) => {
                    dialog.close();
                },
                Err(message) => {
                    let imp = dialog.imp();
                    imp.banner.set_title(&message);
                    imp.banner.set_revealed(true);
                    imp.add.set_sensitive(true);
                },
            }
        ));

        let plugin = Plugin {
            name: imp.name.text().trim().to_string(),
            transport,
            timeout: imp.timeout.value() as i64,
            disabled: imp.disabled.get(),
            env: env_map,
            args: plugin_args,
        };
        if let Some(on_submit) = imp.on_submit.borrow().as_ref() {
            on_submit(plugin, done);
        }
    }

    fn prefill(&self, initial: Plugin) {
        let imp = self.imp();
        imp.name.set_text(&initial.name);
        // Names are immutable because updates address the plugin by name.
        imp.name.set_editable(false);
        imp.timeout.set_value(initial.timeout as f64);
        if !initial.env.is_empty() {
            imp.env
                .set_text(&serde_json::to_string(&initial.env).unwrap_or_default());
        }
        match &initial.args {
            PluginArgs::Local {
                command,
                args: arg_list,
            } => {
                imp.kind.set_selected(0);
                imp.command.set_text(command);
                if !arg_list.is_empty() {
                    imp.args
                        .set_text(&serde_json::to_string(arg_list).unwrap_or_default());
                }
            },
            PluginArgs::Remote { url, requires_auth } => {
                imp.kind.set_selected(1);
                imp.url.set_text(url);
                imp.requires_auth.set_active(*requires_auth);
            },
        }
        self.validate();
    }
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

/// Parse the optional args field; empty means no args.
fn parse_args(text: &str) -> Result<Vec<String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(Vec::new())
    } else {
        serde_json::from_str(text)
    }
}

/// Parse the optional env field; empty means no env.
fn parse_env(text: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(HashMap::new())
    } else {
        serde_json::from_str(text)
    }
}
