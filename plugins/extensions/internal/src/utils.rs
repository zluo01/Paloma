use log::error;

#[cfg(target_os = "linux")]
pub(crate) fn copy_to_clipboard(text: &str) {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};

    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("copy to clipboard failed: {e}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_to_clipboard(text: &str) {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;

    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let text = NSString::from_str(text);
        if !pasteboard.setString_forType(&text, unsafe { NSPasteboardTypeString }) {
            error!("copy to clipboard failed");
        }
    });
}
