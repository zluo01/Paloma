use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
#[cfg(target_os = "linux")]
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use log::{debug, error};
#[cfg(target_os = "macos")]
use objc2::rc::autoreleasepool;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use scry_extension_base::{Capability, SearchHandler};
use scry_extension_protocol::v1::{
    Action, CapabilityIcon, Hide, Item, run_action_response::Behavior,
};

use crate::utils::copy_to_clipboard;

const HISTORY_LIMIT: usize = 100;
const RESPAWN_BACKOFF: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// De-facto standard markers (<https://nspasteboard.org>) set by password
/// managers and similar tools on entries that must stay out of history.
#[cfg(target_os = "macos")]
const PRIVATE_TYPE_MARKERS: &[&str] = &[
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];
const ICON_NAME: &str = "edit-paste";

const COPY_ACTION_LABEL: &str = "Copy";
const REMOVE_ACTION_LABEL: &str = "Remove";
const SUPPORTED_ACTION_LABELS: &[&str] = &[COPY_ACTION_LABEL, REMOVE_ACTION_LABEL];

pub struct Clipboard {
    history: Arc<RwLock<VecDeque<String>>>,
}

impl Capability for Clipboard {
    fn id(&self) -> &str {
        "Clipboard"
    }

    fn description(&self) -> &str {
        "Browse and reuse clipboard history."
    }

    fn search_handler(&self) -> Option<&dyn SearchHandler> {
        Some(self)
    }
}

impl SearchHandler for Clipboard {
    fn search(&self, input: &str) -> Vec<Item> {
        let words: Vec<String> = input.split_whitespace().map(str::to_lowercase).collect();
        let entries = self.history.read().unwrap();

        entries
            .iter()
            .filter(|e| matches_all_words(e, &words))
            .map(|e| build_item(e))
            .collect()
    }

    fn run_search_action(&self, action: Action) -> Behavior {
        let Some(text) = action.params.into_iter().next() else {
            error!("action with no payload");
            return Behavior::Hide(Hide {});
        };

        match action.label.as_str() {
            COPY_ACTION_LABEL => copy_to_clipboard(&text),
            REMOVE_ACTION_LABEL => self.remove_entry(&text),
            other => {
                error!("unknown action label: {other}");
            },
        };

        Behavior::Hide(Hide {})
    }
}

impl Clipboard {
    pub fn new() -> Self {
        let history: Arc<RwLock<VecDeque<String>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(HISTORY_LIMIT)));
        let history_for_watcher = Arc::clone(&history);

        thread::Builder::new()
            .name("scry-clipboard".into())
            .spawn(move || watcher_loop(history_for_watcher))
            .expect("spawn clipboard watcher thread");

        Self { history }
    }

    fn remove_entry(&self, text: &str) {
        let mut g = self.history.write().unwrap();
        g.retain(|e| e != text);
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

fn watcher_loop(history: Arc<RwLock<VecDeque<String>>>) {
    loop {
        match watch_clipboard(&history) {
            Ok(()) => debug!("watcher exited cleanly; respawning"),
            Err(e) => error!("watcher error: {e}; respawning"),
        }
        thread::sleep(RESPAWN_BACKOFF);
    }
}

#[cfg(target_os = "linux")]
fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
    // wl-paste --watch runs the inner command on every clipboard change.
    // The inner command writes the new selection followed by a NUL byte so
    // we can frame entries that themselves contain newlines.
    let mut child = Command::new("wl-paste")
        .args(["--watch", "sh", "-c", "wl-paste --no-newline; printf '\\0'"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(4096);

    loop {
        buf.clear();
        match reader.read_until(0u8, &mut buf) {
            Ok(0) => {
                let _ = child.wait();
                return Ok(());
            },
            Ok(_) => {
                if buf.last() == Some(&0u8) {
                    buf.pop();
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                if !text.trim().is_empty() {
                    push_entry(history, text);
                }
            },
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            },
        }
    }
}

// macOS has no clipboard-change notification API; poll the pasteboard's
// change counter (an in-process message send, no data transfer) and read
// the contents only when it moves.
#[cfg(target_os = "macos")]
fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
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

// Skips non-text contents and anything flagged with a privacy marker.
#[cfg(target_os = "macos")]
fn recordable_text(pasteboard: &NSPasteboard) -> Option<String> {
    let types = pasteboard.types()?;
    if has_private_marker(types.iter().map(|t| t.to_string())) {
        return None;
    }
    pasteboard
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string())
}

#[cfg(target_os = "macos")]
fn has_private_marker<S: AsRef<str>>(types: impl IntoIterator<Item = S>) -> bool {
    types
        .into_iter()
        .any(|t| PRIVATE_TYPE_MARKERS.contains(&t.as_ref()))
}

fn push_entry(history: &RwLock<VecDeque<String>>, text: String) {
    let mut g = history.write().unwrap();
    g.retain(|e| *e != text);
    g.push_front(text);
    while g.len() > HISTORY_LIMIT {
        g.pop_back();
    }
}

fn matches_all_words(entry: &str, words: &[String]) -> bool {
    let text = entry.to_lowercase();
    words.iter().all(|word| text.contains(word))
}

fn build_item(text: &str) -> Item {
    Item {
        title: text.to_owned(),
        subtitle: None,
        icon: Some(CapabilityIcon::name(ICON_NAME)),
        actions: SUPPORTED_ACTION_LABELS
            .iter()
            .map(|label| Action {
                label: (*label).into(),
                params: vec![text.to_owned()],
                primary: *label == COPY_ACTION_LABEL,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(query: &str) -> Vec<String> {
        query.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn matches_words_in_any_order() {
        assert!(matches_all_words(
            "my token for github.com",
            &words("github token")
        ));
    }

    #[test]
    fn rejects_entries_missing_a_word() {
        assert!(!matches_all_words(
            "my token for github.com",
            &words("github secret")
        ));
    }

    #[test]
    fn matching_ignores_case() {
        assert!(matches_all_words("GitHub Token", &words("gItHuB")));
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(matches_all_words("anything", &words("   ")));
    }

    #[test]
    fn push_entry_dedupes_and_bumps_to_front() {
        let h = RwLock::new(VecDeque::new());
        push_entry(&h, "a".into());
        push_entry(&h, "b".into());
        push_entry(&h, "a".into());
        let g = h.read().unwrap();
        let texts: Vec<_> = g.iter().map(|e| e.as_str()).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }

    #[test]
    fn push_entry_caps_history_at_limit() {
        let h = RwLock::new(VecDeque::new());
        for i in 0..(HISTORY_LIMIT + 10) {
            push_entry(&h, format!("entry-{i}"));
        }
        assert_eq!(h.read().unwrap().len(), HISTORY_LIMIT);
    }

    // Read-only smoke test: the pasteboard is usable from a non-main
    // thread (tests run off-main), which is where the watcher lives.
    #[cfg(target_os = "macos")]
    #[test]
    fn pasteboard_reads_off_main_thread() {
        let pasteboard = NSPasteboard::generalPasteboard();
        assert!(pasteboard.changeCount() >= 0);
        let _ = recordable_text(&pasteboard);
    }

    #[cfg(target_os = "macos")]
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
