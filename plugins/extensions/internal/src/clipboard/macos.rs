use log::error;

pub fn copy_to_clipboard(text: &str) {
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
