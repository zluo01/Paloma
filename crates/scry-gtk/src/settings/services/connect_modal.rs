// Connect-flow dialog, opened from a provider row's "Connect" button.
// Stages: loading → device-code challenge → success (auto-close) or
// error (manual close).

use std::{rc::Rc, sync::Arc, time::Duration};

use adw::{prelude::*, Dialog, Spinner, ToolbarView};
use gtk4::{gio, glib, Align, Box as GtkBox, Button, Label, Orientation};
use libadwaita as adw;
use log::warn;
use scry_core::AppContext;
use scry_provider::{Connection, ProviderId};

use crate::runtime;

/// Open the dialog and run the connect flow. Returns immediately; the flow
/// runs asynchronously. `on_connected` fires only on success, right before
/// the dialog auto-closes.
pub fn open(
    parent: &impl IsA<gtk4::Widget>,
    app: Arc<AppContext>,
    provider_id: ProviderId,
    provider_name: &str,
    on_connected: Rc<dyn Fn()>,
) {
    let dialog = Dialog::builder()
        .title(format!("Connect — {provider_name}"))
        .content_width(420)
        .content_height(300)
        .build();

    let body = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(28)
        .margin_end(28)
        .valign(Align::Center)
        .vexpand(true)
        .build();

    let view = ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&body));
    dialog.set_child(Some(&view));
    show_loading(&body);

    let dialog_for_flow = dialog.clone();
    let body_for_flow = body.clone();
    glib::MainContext::default().spawn_local(async move {
        let dialog = dialog_for_flow;
        let body = body_for_flow;

        let init = runtime::spawn({
            let app = app.clone();
            async move { app.connect.init(provider_id).await }
        })
        .await;

        let payload = match init {
            Ok(Connection::DeviceCode {
                verification_uri,
                user_code,
                transaction_payload,
            }) => {
                show_challenge(&body, verification_uri, &user_code);
                launch_url(verification_uri);
                Connection::DeviceCode {
                    verification_uri,
                    user_code,
                    transaction_payload,
                }
            },
            Ok(_) => {
                return show_error(
                    &body,
                    &dialog,
                    "Provider did not return a device-code challenge.",
                )
            },
            Err(e) => return show_error(&body, &dialog, &e.to_string()),
        };

        let finalize =
            runtime::spawn(async move { app.connect.finalize(provider_id, payload).await }).await;
        match finalize {
            Ok(_) => {
                on_connected();
                show_success(&body);
                glib::timeout_add_local_once(Duration::from_millis(800), move || {
                    dialog.close();
                });
            },
            Err(e) => show_error(&body, &dialog, &e.to_string()),
        }
    });

    dialog.present(Some(parent));
}

fn clear(body: &GtkBox) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
}

fn show_loading(body: &GtkBox) {
    clear(body);
    let spinner = Spinner::new();
    spinner.set_halign(Align::Center);
    spinner.set_size_request(32, 32);
    body.append(&spinner);
    body.append(
        &Label::builder()
            .label("Connecting…")
            .halign(Align::Center)
            .css_classes(["dim-label"])
            .build(),
    );
}

fn show_challenge(body: &GtkBox, verification_uri: &str, user_code: &str) {
    clear(body);

    body.append(
        &Label::builder()
            .label("Enter this code in your browser")
            .halign(Align::Center)
            .css_classes(["heading"])
            .build(),
    );

    let code = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();
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
    body.append(&code);

    body.append(
        &Label::builder()
            .label(verification_uri)
            .selectable(true)
            .halign(Align::Center)
            .css_classes(["caption", "dim-label", "monospace"])
            .build(),
    );

    let open = Button::builder()
        .label("Open in browser")
        .halign(Align::Center)
        .css_classes(["pill"])
        .build();
    let uri = verification_uri.to_string();
    open.connect_clicked(move |_| launch_url(&uri));
    body.append(&open);

    body.append(
        &Label::builder()
            .label("Waiting for approval…")
            .halign(Align::Center)
            .css_classes(["caption", "dim-label"])
            .build(),
    );
}

fn show_success(body: &GtkBox) {
    clear(body);
    body.append(
        &Label::builder()
            .label("✓")
            .halign(Align::Center)
            .css_classes(["scry-connect-check"])
            .build(),
    );
    body.append(
        &Label::builder()
            .label("Connected")
            .halign(Align::Center)
            .css_classes(["heading"])
            .build(),
    );
}

fn show_error(body: &GtkBox, dialog: &Dialog, message: &str) {
    clear(body);

    body.append(
        &Label::builder()
            .label("Connection failed")
            .halign(Align::Center)
            .css_classes(["heading"])
            .build(),
    );
    body.append(
        &Label::builder()
            .label(message)
            .halign(Align::Center)
            .wrap(true)
            .css_classes(["error"])
            .build(),
    );

    let close = Button::builder()
        .label("Close")
        .halign(Align::Center)
        .css_classes(["pill"])
        .build();
    let dialog = dialog.clone();
    close.connect_clicked(move |_| {
        dialog.close();
    });
    body.append(&close);
}

/// Best-effort open the URL in the user's default browser.
fn launch_url(uri: &str) {
    let launcher = gio::AppLaunchContext::new();
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, Some(&launcher)) {
        warn!("failed to open {uri}: {e}");
    }
}
