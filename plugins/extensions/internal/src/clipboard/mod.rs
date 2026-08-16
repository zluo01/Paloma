#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::{
    collections::VecDeque,
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

#[cfg(target_os = "linux")]
pub use linux::copy_to_clipboard;
use log::{debug, error};
#[cfg(target_os = "macos")]
pub use macos::copy_to_clipboard;
use paloma_extension_base::{Capability, SearchHandler};
use paloma_extension_protocol::v1::{
    Action, CapabilityIcon, Hide, Item, run_action_response::Behavior,
};
#[cfg(windows)]
pub use windows::copy_to_clipboard;

#[cfg(target_os = "linux")]
use crate::clipboard::linux::watch_clipboard;
#[cfg(target_os = "macos")]
use crate::clipboard::macos::watch_clipboard;
#[cfg(windows)]
use crate::clipboard::windows::watch_clipboard;

const HISTORY_LIMIT: usize = 100;
const RESPAWN_BACKOFF: Duration = Duration::from_secs(2);
#[cfg(target_os = "linux")]
const ICON_NAME: &str = "edit-paste";
#[cfg(target_os = "macos")]
const ICON_NAME: &str = "doc.on.clipboard";
#[cfg(windows)]
const ICON_NAME: &str = "\u{E77F}";

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
            .name("paloma-clipboard".into())
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

fn push_entry(history: &RwLock<VecDeque<String>>, text: String) {
    if text.trim().is_empty() {
        return;
    }
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

/// Collapse whitespace to a single line for display.
fn minimize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_item(text: &str) -> Item {
    Item {
        title: minimize_text(text),
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

    const PRETTY_JSON: &str = r#"{
        "name": "paloma",
        "tags": [
            "launcher",
            "clipboard"
        ]
    }"#;

    #[test]
    fn item_title_minimizes_json_to_one_line() {
        let item = build_item(PRETTY_JSON);
        assert_eq!(
            item.title,
            r#"{ "name": "paloma", "tags": [ "launcher", "clipboard" ] }"#
        );
    }

    #[test]
    fn item_title_minimizes_html_to_one_line() {
        let html = r#"<div class="card">
            <p>Hello, <b>world</b></p>
        </div>"#;
        let item = build_item(html);
        assert_eq!(
            item.title,
            r#"<div class="card"> <p>Hello, <b>world</b></p> </div>"#
        );
    }

    #[test]
    fn item_title_trims_surrounding_whitespace() {
        let item = build_item("  hello world \n");
        assert_eq!(item.title, "hello world");
    }

    #[test]
    fn item_title_collapses_crlf_and_tabs() {
        let item = build_item("first\r\nsecond\tthird");
        assert_eq!(item.title, "first second third");
    }

    #[test]
    fn push_entry_ignores_whitespace_only_text() {
        let h = RwLock::new(VecDeque::new());
        push_entry(&h, " \r\n\t ".into());
        assert!(h.read().unwrap().is_empty());
    }

    #[test]
    fn item_title_keeps_single_line_text_unchanged() {
        let item = build_item(r#"{"name":"paloma"}"#);
        assert_eq!(item.title, r#"{"name":"paloma"}"#);
    }

    #[test]
    fn item_carries_the_platform_icon_name() {
        use paloma_extension_protocol::v1::capability_icon::Icon;

        let item = build_item("text");
        assert_eq!(item.icon.unwrap().icon, Some(Icon::Name(ICON_NAME.into())));
    }

    #[test]
    fn item_actions_keep_full_original_text() {
        let item = build_item(PRETTY_JSON);
        assert!(!item.actions.is_empty());
        assert!(
            item.actions
                .iter()
                .all(|a| a.params == [PRETTY_JSON.to_owned()])
        );
    }
}
