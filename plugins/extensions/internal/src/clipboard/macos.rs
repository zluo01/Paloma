use std::{collections::VecDeque, sync::RwLock, thread, time::Duration};

use log::error;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

use crate::clipboard::push_entry;

const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// De-facto standard markers (<https://nspasteboard.org>) set by password
/// managers and similar tools on entries that must stay out of history.
const PRIVATE_TYPE_MARKERS: &[&str] = &[
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

pub(super) fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // Start from the current count so pre-launch contents are not recorded.
    let mut last_change = pasteboard.changeCount();

    loop {
        // The pool drains the objects the pasteboard calls autorelease each
        // tick; without it they accumulate for the lifetime of this
        // never-exiting thread.
        autoreleasepool(|_| {
            let change = pasteboard.changeCount();
            if change == last_change {
                return;
            }
            let text = recordable_text(&pasteboard);
            // Discard the sample if the pasteboard moved mid-read — the
            // marker check and the text could belong to different entries
            // (e.g. a concealed password landing between the two calls).
            // The new entry is examined on the next tick.
            if pasteboard.changeCount() != change {
                return;
            }
            last_change = change;
            if let Some(text) = text
                && !text.trim().is_empty()
            {
                push_entry(history, text);
            }
        });
        thread::sleep(POLL_INTERVAL);
    }
}

fn recordable_text(pasteboard: &NSPasteboard) -> Option<String> {
    let types = pasteboard.types()?;
    if has_private_marker(types.iter().map(|t| t.to_string())) {
        return None;
    }
    pasteboard
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string())
}

fn has_private_marker<S: AsRef<str>>(types: impl IntoIterator<Item = S>) -> bool {
    types
        .into_iter()
        .any(|t| PRIVATE_TYPE_MARKERS.contains(&t.as_ref()))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    // Read-only smoke test: the pasteboard is usable from a non-main
    // thread (tests run off-main), which is where the watcher lives.
    #[test]
    fn pasteboard_reads_off_main_thread() {
        let pasteboard = NSPasteboard::generalPasteboard();
        assert!(pasteboard.changeCount() >= 0);
        let _ = recordable_text(&pasteboard);
    }

    #[test]
    fn private_pasteboard_markers_are_detected() {
        assert!(has_private_marker([
            "public.utf8-plain-text",
            "org.nspasteboard.ConcealedType",
        ]));
        assert!(has_private_marker(["org.nspasteboard.TransientType"]));
        assert!(!has_private_marker(["public.utf8-plain-text"]));
    }
}
