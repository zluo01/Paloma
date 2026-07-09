use gtk4::{Align, Image, gio};
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, PreferencesGroup,
    prelude::{AdwDialogExt, AlertDialogExt, PreferencesGroupExt},
};
use log::warn;

pub(super) fn group_is_empty(group: &PreferencesGroup) -> bool {
    group.row(0).is_none()
}

pub(super) fn unhealthy_icon(error: Option<&str>) -> Image {
    Image::builder()
        .icon_name("help-about-symbolic")
        .tooltip_text(error.unwrap_or("unknown error"))
        .css_classes(["scry-unhealthy-icon"])
        .valign(Align::Center)
        .build()
}

pub(super) fn placeholder(text: &str) -> ActionRow {
    ActionRow::builder()
        .title(text)
        .css_classes(["dim-label"])
        .build()
}

pub(super) fn launch_url(uri: &str) {
    let launcher = gio::AppLaunchContext::new();
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, Some(&launcher)) {
        warn!("failed to open {uri}: {e}");
    }
}

pub(super) fn show_error_dialog(window: &ApplicationWindow, heading: &str, message: &str) {
    let dialog = AlertDialog::builder()
        .heading(heading)
        .body(message)
        .close_response("close")
        .build();
    dialog.add_response("close", "Close");
    dialog.present(Some(window));
}
