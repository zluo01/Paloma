mod model;

use std::{collections::HashSet, rc::Rc, time::Duration};

use gtk4::{glib, prelude::*, subclass::prelude::*};
use libadwaita::prelude::*;
use scry_core::Plugin;

use self::model::{Command, Kind, Model, Msg};

const VALIDATE_DEBOUNCE: Duration = Duration::from_millis(300);

pub(super) type SaveFinished = Rc<dyn Fn(Result<(), String>)>;
pub(super) type SavePlugin = Rc<dyn Fn(Plugin, SaveFinished)>;

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk4::{Button, CompositeTemplate, glib, subclass::prelude::*};
    use libadwaita::{Banner, ComboRow, EntryRow, SpinRow, SwitchRow, subclass::prelude::*};

    use super::{Model, SavePlugin};

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

        pub(super) model: RefCell<Model>,
        pub save_plugin: RefCell<Option<SavePlugin>>,
        /// True while prefilling, so programmatic edits don't dispatch.
        pub suppress: Cell<bool>,
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
            self.obj().connect_signals();
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

pub(super) fn open(
    parent: &impl IsA<gtk4::Widget>,
    taken: HashSet<String>,
    initial: Option<Plugin>,
    save_plugin: SavePlugin,
) {
    PluginDialog::new(taken, initial, save_plugin).present(Some(parent));
}

impl PluginDialog {
    fn new(taken: HashSet<String>, initial: Option<Plugin>, save_plugin: SavePlugin) -> Self {
        let dialog: Self = glib::Object::new();
        let editing = initial.is_some();
        let imp = dialog.imp();
        imp.save_plugin.replace(Some(save_plugin));

        dialog.set_title(if editing { "Edit Plugin" } else { "Add Plugin" });
        imp.add.set_label(if editing { "Save" } else { "Add" });

        let model = if let Some(initial) = initial {
            Model::from_plugin(taken, initial)
        } else {
            Model {
                taken,
                ..Model::default()
            }
        };
        imp.model.replace(model);

        dialog.prefill_widgets();
        dialog.render();
        dialog
    }

    fn connect_signals(&self) {
        let imp = self.imp();

        imp.banner
            .connect_button_clicked(|banner| banner.set_revealed(false));

        imp.cancel.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.dispatch(Msg::CancelClicked)
        ));
        imp.add.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| {
                let imp = dialog.imp();
                dialog.dispatch(Msg::SubmitClicked {
                    timeout: imp.timeout.value() as i64,
                    requires_auth: imp.requires_auth.is_active(),
                });
            }
        ));
        imp.kind.connect_selected_notify(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |row| {
                if dialog.imp().suppress.get() {
                    return;
                }
                let kind = if row.selected() == 0 {
                    Kind::Local
                } else {
                    Kind::Remote
                };
                dialog.dispatch(Msg::KindChanged(kind));
            }
        ));

        type FieldBinding<'a> = (&'a libadwaita::EntryRow, fn(String) -> Msg);

        // Each text field maps its current contents to the matching message.
        let fields: [FieldBinding<'_>; 5] = [
            (&imp.name, Msg::NameChanged),
            (&imp.command, Msg::CommandChanged),
            (&imp.args, Msg::ArgsChanged),
            (&imp.url, Msg::UrlChanged),
            (&imp.env, Msg::EnvChanged),
        ];
        for (row, to_msg) in fields {
            row.connect_changed(glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                move |row| {
                    if dialog.imp().suppress.get() {
                        return;
                    }
                    dialog.dispatch(to_msg(row.text().into()));
                }
            ));
        }

        // Dismissing the dialog cancels pending validation against widgets that
        // are no longer visible.
        self.connect_closed(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.cancel_pending()
        ));
    }

    fn dispatch(&self, msg: Msg) {
        let commands = self.imp().model.borrow_mut().update(msg);
        for command in commands {
            self.run(command);
        }
    }

    fn run(&self, command: Command) {
        let imp = self.imp();
        match command {
            Command::RenderForm => self.render(),
            Command::ScheduleValidation => self.schedule_validate(),
            Command::PersistPlugin(plugin) => {
                let save_finished = self.save_finished();
                if let Some(save_plugin) = imp.save_plugin.borrow().as_ref() {
                    save_plugin(plugin, save_finished);
                }
            },
            Command::ShowErrorBanner(message) => {
                imp.banner.set_title(&message);
                imp.banner.set_revealed(true);
            },
            Command::CloseDialog => {
                self.close();
            },
        }
    }

    fn render(&self) {
        let imp = self.imp();
        let form = imp.model.borrow();

        let local = form.kind == Kind::Local;
        imp.command.set_visible(local);
        imp.args.set_visible(local);
        imp.url.set_visible(!local);
        imp.requires_auth.set_visible(!local);
        imp.cancel.set_sensitive(!form.submitting);
        self.set_form_sensitive(!form.submitting);

        if form.submitting {
            imp.add.set_sensitive(false);
            imp.banner.set_revealed(false);
            return;
        }
        if !form.settled {
            imp.add.set_sensitive(false);
            self.clear_flags();
            return;
        }

        let v = form.validate();
        flag(&imp.name.get(), v.name);
        flag(&imp.args.get(), v.args);
        flag(&imp.url.get(), v.url);
        flag(&imp.env.get(), v.env);
        imp.add.set_sensitive(v.ok);
    }

    fn set_form_sensitive(&self, sensitive: bool) {
        let imp = self.imp();
        imp.name.set_sensitive(sensitive);
        imp.kind.set_sensitive(sensitive);
        imp.command.set_sensitive(sensitive);
        imp.args.set_sensitive(sensitive);
        imp.url.set_sensitive(sensitive);
        imp.requires_auth.set_sensitive(sensitive);
        imp.timeout.set_sensitive(sensitive);
        imp.env.set_sensitive(sensitive);
    }

    fn clear_flags(&self) {
        let imp = self.imp();
        for row in [&imp.name, &imp.args, &imp.url, &imp.env] {
            flag(&row.get(), None);
        }
    }

    /// Push the form's stored values into the widgets without re-dispatching.
    fn prefill_widgets(&self) {
        let imp = self.imp();
        imp.suppress.set(true);

        let form = imp.model.borrow();
        if form.editing {
            imp.name.set_text(&form.name);
            // Names are immutable because updates address the plugin by name.
            imp.name.set_editable(false);
        }
        imp.kind
            .set_selected(if form.kind == Kind::Local { 0 } else { 1 });
        imp.command.set_text(&form.command);
        imp.args.set_text(&form.args);
        imp.url.set_text(&form.url);
        imp.requires_auth.set_active(form.requires_auth);
        if form.timeout > 0 {
            imp.timeout.set_value(form.timeout as f64);
        }
        imp.env.set_text(&form.env);
        drop(form);

        imp.suppress.set(false);
    }

    fn schedule_validate(&self) {
        let imp = self.imp();
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
                    dialog.dispatch(Msg::ValidationDebounceElapsed);
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

    fn save_finished(&self) -> SaveFinished {
        Rc::new(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            move |result| dialog.dispatch(Msg::PluginSaveFinished(result))
        ))
    }
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
