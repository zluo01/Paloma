//! Provider connection dialog.
//!
//! The dialog owns the GTK stack for loading, device-code challenge, success,
//! and error states. It starts the backend connect flow, aborts it on close,
//! and reports success right before the success state auto-closes.

use std::{rc::Rc, sync::Arc, time::Duration};

use gtk4::{Label, gio, glib, prelude::*, subclass::prelude::*};
use libadwaita::prelude::*;
use log::warn;
use scry_core::{AppContext, Connection, ProviderId};

use crate::runtime;

/// Time to leave the success state visible before auto-close.
const CLOSE_DELAY: Duration = Duration::from_millis(800);

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use gtk4::{
        Box as GtkBox, Button, CompositeTemplate, Label, Stack, glib, prelude::*,
        subclass::prelude::*,
    };
    use libadwaita::{prelude::*, subclass::prelude::*};
    use scry_core::{AppContext, ProviderId};

    #[derive(CompositeTemplate, Default)]
    #[template(file = "connect_dialog.ui")]
    pub struct ConnectDialog {
        #[template_child]
        pub stack: TemplateChild<Stack>,
        /// Device-code cells rendered from the current challenge.
        #[template_child]
        pub code: TemplateChild<GtkBox>,
        #[template_child]
        pub uri_label: TemplateChild<Label>,
        #[template_child]
        pub open: TemplateChild<Button>,
        #[template_child]
        pub error_message: TemplateChild<Label>,
        #[template_child]
        pub error_close: TemplateChild<Button>,

        pub app: OnceCell<Arc<AppContext>>,
        pub provider_id: OnceCell<ProviderId>,
        pub on_connected: RefCell<Option<Rc<dyn Fn()>>>,
        /// Verification URL reused by the "Open in browser" button.
        pub uri: RefCell<String>,
        /// Running backend flow, aborted when the dialog closes.
        pub flow: RefCell<Option<glib::JoinHandle<()>>>,
        /// Success auto-close timeout, cancelled on manual close.
        pub close_timeout: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ConnectDialog {
        const NAME: &'static str = "ScryConnectDialog";
        type Type = super::ConnectDialog;
        type ParentType = libadwaita::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ConnectDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.open.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| super::launch_url(obj.imp().uri.borrow().as_str())
            ));
            self.error_close.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));

            // Closing the dialog cancels async work so callbacks cannot update
            // a dismissed dialog.
            obj.connect_closed(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    let imp = obj.imp();
                    if let Some(flow) = imp.flow.borrow_mut().take() {
                        flow.abort();
                    }
                    if let Some(id) = imp.close_timeout.borrow_mut().take() {
                        id.remove();
                    }
                }
            ));
        }
    }

    impl WidgetImpl for ConnectDialog {}
    impl AdwDialogImpl for ConnectDialog {}
}

glib::wrapper! {
    pub struct ConnectDialog(ObjectSubclass<imp::ConnectDialog>)
        @extends libadwaita::Dialog, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

/// Open the dialog and start the async connection flow.
///
/// Returns immediately. `on_connected` fires only on success, before the
/// success state auto-closes.
pub(super) fn open(
    parent: &impl IsA<gtk4::Widget>,
    app: Arc<AppContext>,
    provider_id: ProviderId,
    on_connected: Rc<dyn Fn()>,
) {
    ConnectDialog::new(app, provider_id, on_connected).present(Some(parent));
}

impl ConnectDialog {
    fn new(app: Arc<AppContext>, provider_id: ProviderId, on_connected: Rc<dyn Fn()>) -> Self {
        let dialog: Self = glib::Object::new();
        dialog.set_title(&format!("Connect — {provider_id}"));
        let imp = dialog.imp();
        let _ = imp.app.set(app);
        let _ = imp.provider_id.set(provider_id);
        imp.on_connected.replace(Some(on_connected));
        imp.stack.set_visible_child_name("loading");
        dialog.start_flow();
        dialog
    }

    fn start_flow(&self) {
        let imp = self.imp();
        let app = imp.app.get().expect("app set in new").clone();
        let provider_id = *imp.provider_id.get().expect("provider set in new");

        let handle =
            glib::MainContext::default().spawn_local(glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                async move {
                    let init = runtime::spawn({
                        let app = app.clone();
                        async move { app.init_connection(provider_id).await }
                    })
                    .await;

                    match init {
                        Ok(Connection::DeviceCode {
                            verification_uri,
                            user_code,
                            transaction_payload,
                        }) => {
                            dialog.show_challenge(verification_uri, &user_code);
                            let payload = Connection::DeviceCode {
                                verification_uri,
                                user_code,
                                transaction_payload,
                            };
                            let finalize = runtime::spawn(async move {
                                app.finalize_connection(provider_id, payload).await
                            })
                            .await;
                            match finalize {
                                Ok(_) => dialog.finish_success(),
                                Err(e) => dialog.show_error(&e.to_string()),
                            }
                        },
                        // Core supports these variants, but this dialog only
                        // implements device-code flows.
                        Ok(Connection::BrowserRedirect { .. }) => dialog
                            .show_error("Browser sign-in for this provider isn't supported yet."),
                        Ok(Connection::ManualInput { .. }) => dialog
                            .show_error("Pasting a key for this provider isn't supported yet."),
                        Ok(Connection::None) => {
                            dialog.show_error("This provider doesn't require a connection.")
                        },
                        Err(e) => dialog.show_error(&e.to_string()),
                    }
                }
            ));
        imp.flow.replace(Some(handle));
    }

    fn show_challenge(&self, verification_uri: &str, user_code: &str) {
        let imp = self.imp();
        imp.uri.replace(verification_uri.to_string());

        let code = imp.code.get();
        while let Some(child) = code.first_child() {
            code.remove(&child);
        }
        for ch in user_code.chars() {
            let cell = Label::new(Some(&ch.to_string()));
            if ch == '-' {
                cell.set_css_classes(&["scry-otp-sep", "dim-label"]);
            } else {
                cell.set_css_classes(&["scry-otp-cell", "monospace"]);
                cell.set_size_request(36, 44);
            }
            code.append(&cell);
        }

        imp.uri_label.set_label(verification_uri);
        launch_url(verification_uri);
        imp.stack.set_visible_child_name("challenge");
    }

    fn finish_success(&self) {
        if let Some(on_connected) = self.imp().on_connected.borrow().as_ref() {
            on_connected();
        }
        self.imp().stack.set_visible_child_name("success");
        let id = glib::timeout_add_local_once(
            CLOSE_DELAY,
            glib::clone!(
                #[weak(rename_to = dialog)]
                self,
                move || {
                    dialog.close();
                }
            ),
        );
        self.imp().close_timeout.replace(Some(id));
    }

    fn show_error(&self, message: &str) {
        let imp = self.imp();
        imp.error_message.set_label(message);
        imp.stack.set_visible_child_name("error");
    }
}

fn launch_url(uri: &str) {
    let launcher = gio::AppLaunchContext::new();
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, Some(&launcher)) {
        warn!("failed to open {uri}: {e}");
    }
}
