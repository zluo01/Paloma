use std::{rc::Rc, time::Duration};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, Stack, StackTransitionType, glib, prelude::*,
};
use libadwaita::{
    Dialog, EntryRow, HeaderBar, PasswordEntryRow, PreferencesGroup, Spinner, ToolbarView,
    prelude::*,
};
use paloma_core::{ProviderAuthMethod, ProviderBackendId};

use crate::widgets::settings::{helper::launch_url, services::model::Msg};

/// Time to leave the success state visible before auto-close.
const CLOSE_DELAY: Duration = Duration::from_millis(800);

pub(super) struct ConnectionDialog {
    dialog: Dialog,
    stack: Stack,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl ConnectionDialog {
    pub(super) fn new(
        provider_backend_id: ProviderBackendId,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let stack = Stack::builder()
            .vexpand(true)
            .transition_type(StackTransitionType::Crossfade)
            .build();

        let toolbar = ToolbarView::new();
        toolbar.add_top_bar(&HeaderBar::new());
        toolbar.set_content(Some(&stack));

        let dialog = Dialog::builder()
            .title(format!("Connect — {provider_backend_id}"))
            .content_width(420)
            .content_height(360)
            .child(&toolbar)
            .build();

        let tx = dispatcher.clone();
        dialog.connect_closed(move |_| {
            let _ = tx.unbounded_send(Msg::DialogClosed(provider_backend_id.clone()));
        });

        Self {
            dialog,
            stack,
            dispatcher,
        }
    }

    pub(super) fn show(&self, parent: &impl IsA<gtk4::Widget>) {
        self.show_loading();
        self.dialog.present(Some(parent));
    }

    pub(super) fn close(&self) {
        self.dialog.close();
    }

    pub(super) fn show_loading(&self) {
        let loading_view = loading_page();
        self.set_visible(&loading_view);
    }

    pub(super) fn show_challenge(&self, verification_uri: &str, user_code: &str) {
        let challenge_view = challenge_page(verification_uri, user_code);
        self.set_visible(&challenge_view);
        launch_url(verification_uri);
    }

    pub(super) fn show_manual(
        &self,
        provider_backend_id: ProviderBackendId,
        instructions_url: Option<String>,
    ) {
        let (manual_view, key_entry) = manual_page(
            provider_backend_id,
            instructions_url,
            self.dispatcher.clone(),
        );
        self.set_visible(&manual_view);
        key_entry.grab_focus();
    }

    pub(super) fn show_oauth(
        &self,
        provider_backend_id: ProviderBackendId,
        authorization_url: String,
    ) {
        launch_url(&authorization_url);
        let (oauth_view, code_entry) = oauth_page(
            provider_backend_id,
            authorization_url,
            self.dispatcher.clone(),
        );
        self.set_visible(&oauth_view);
        code_entry.grab_focus();
    }

    pub(super) fn show_success(&self) {
        self.set_visible(&success_page());

        glib::timeout_add_local_once(
            CLOSE_DELAY,
            glib::clone!(
                #[weak(rename_to = dialog)]
                self.dialog,
                move || {
                    dialog.close();
                }
            ),
        );
    }

    pub(super) fn show_error(&self, error_msg: &str) {
        let error_view = error_page(error_msg, self.dispatcher.clone());
        self.set_visible(&error_view);
    }

    fn set_visible(&self, page: &GtkBox) {
        self.stack.add_child(page);
        self.stack.set_visible_child(page);
    }
}

fn loading_page() -> GtkBox {
    let body = page(false);
    let spinner = Spinner::builder()
        .halign(Align::Center)
        .width_request(32)
        .height_request(32)
        .build();
    body.append(&spinner);
    let label = Label::builder()
        .label("Connecting…")
        .halign(Align::Center)
        .css_classes(["dim-label"])
        .build();
    body.append(&label);
    body
}

fn challenge_page(verification_uri: &str, user_code: &str) -> GtkBox {
    let body = page(true);
    body.append(&heading("Enter this code in your browser"));

    let code = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();
    for ch in user_code.chars() {
        let cell = Label::new(Some(&ch.to_string()));
        if ch == '-' {
            cell.set_css_classes(&["paloma-otp-sep", "dim-label"]);
        } else {
            cell.set_css_classes(&["paloma-otp-cell", "monospace"]);
            cell.set_size_request(36, 44);
        }
        code.append(&cell);
    }
    body.append(&code);

    let uri_label = Label::builder()
        .label(verification_uri)
        .selectable(true)
        .halign(Align::Center)
        .css_classes(["caption", "dim-label", "monospace"])
        .build();
    body.append(&uri_label);

    let open = Button::builder()
        .label("Open in browser")
        .halign(Align::Center)
        .css_classes(["pill"])
        .build();
    let verification_uri = verification_uri.to_string();
    open.connect_clicked(move |_| {
        launch_url(&verification_uri);
    });
    body.append(&open);

    body.append(&caption("Waiting for approval…"));
    body
}

fn manual_page(
    provider_backend_id: ProviderBackendId,
    instructions_url: Option<String>,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> (GtkBox, PasswordEntryRow) {
    let body = page(true);
    body.append(&heading("Paste your API key"));

    let key_entry = PasswordEntryRow::builder().title("API key").build();
    let group = PreferencesGroup::new();
    group.add(&key_entry);
    body.append(&group);

    if let Some(url) = instructions_url {
        let instructions = Button::builder()
            .label("Get an API key")
            .halign(Align::Center)
            .css_classes(["link"])
            .build();
        instructions.connect_clicked(move |_| launch_url(&url));
        body.append(&instructions);
    }

    let connect = submit_button(
        key_entry.upcast_ref(),
        ProviderAuthMethod::ApiKey,
        provider_backend_id,
        dispatcher,
    );
    body.append(&connect);

    (body, key_entry)
}

fn oauth_page(
    provider_backend_id: ProviderBackendId,
    authorization_url: String,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> (GtkBox, EntryRow) {
    let body = page(true);
    body.append(&heading("Finish browser sign-in"));

    let hint = caption("Paste the returned code or callback URL.");
    hint.set_wrap(true);
    body.append(&hint);

    let browser_entry = EntryRow::builder().title("Authorization code").build();
    let group = PreferencesGroup::new();
    group.add(&browser_entry);
    body.append(&group);

    let open = Button::builder()
        .label("Open in browser")
        .halign(Align::Center)
        .css_classes(["link"])
        .build();
    open.connect_clicked(move |_| launch_url(&authorization_url));
    body.append(&open);

    let connect = submit_button(
        &browser_entry,
        ProviderAuthMethod::BrowserOauth,
        provider_backend_id,
        dispatcher,
    );
    body.append(&connect);

    (body, browser_entry)
}

fn success_page() -> GtkBox {
    let body = page(false);
    let check = Label::builder()
        .label("✓")
        .halign(Align::Center)
        .css_classes(["paloma-connect-check"])
        .build();
    body.append(&check);
    body.append(&heading("Connected"));
    body
}

fn error_page(message: &str, dispatcher: mpsc::UnboundedSender<Msg>) -> GtkBox {
    let body = page(true);
    body.append(&heading("Connection failed"));

    let label = Label::builder()
        .label(message)
        .halign(Align::Center)
        .wrap(true)
        .css_classes(["error"])
        .build();
    body.append(&label);

    let close = Button::builder()
        .label("Close")
        .halign(Align::Center)
        .css_classes(["pill"])
        .build();
    close.connect_clicked(move |_| {
        let _ = dispatcher.unbounded_send(Msg::CloseDialogClicked);
    });
    body.append(&close);

    body
}

fn submit_button(
    entry: &EntryRow,
    provider_auth_method: ProviderAuthMethod,
    provider_backend_id: ProviderBackendId,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> Button {
    let connect = Button::builder()
        .label("Connect")
        .halign(Align::Center)
        .sensitive(false)
        .css_classes(["pill", "suggested-action"])
        .build();

    let submit: Rc<dyn Fn()> = {
        let entry = entry.clone();
        Rc::new(move || {
            let payload = entry.text().trim().to_string();
            if payload.is_empty() {
                return;
            }
            let _ = dispatcher.unbounded_send(Msg::ConnectionSubmitted {
                provider_auth_method,
                provider_backend_id: provider_backend_id.clone(),
                payload,
            });
        })
    };
    let on_click = submit.clone();
    connect.connect_clicked(move |_| on_click());
    entry.connect_entry_activated(move |_| submit());

    let button = connect.clone();
    entry.connect_changed(move |entry| {
        button.set_sensitive(!entry.text().trim().is_empty());
    });

    connect
}

/// Vertically centered page body; `margins` applies the inset shared by the
/// interactive states.
fn page(margins: bool) -> GtkBox {
    let builder = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .valign(Align::Center)
        .vexpand(true);
    let builder = if margins {
        builder
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(28)
            .margin_end(28)
    } else {
        builder
    };
    builder.build()
}

fn heading(text: &str) -> Label {
    Label::builder()
        .label(text)
        .halign(Align::Center)
        .css_classes(["heading"])
        .build()
}

fn caption(text: &str) -> Label {
    Label::builder()
        .label(text)
        .halign(Align::Center)
        .css_classes(["caption", "dim-label"])
        .build()
}
