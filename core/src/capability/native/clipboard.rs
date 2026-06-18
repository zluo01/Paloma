use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use log::{debug, error};
use wl_clipboard_rs::copy::{
    MimeType as CopyMimeType, Options as CopyOptions, Source as CopySource,
};

use crate::capability::{
    Action, ActionOutcome, Capability, CapabilityMeta, IconRef, Item, QueryHandler,
};

const HISTORY_LIMIT: usize = 100;
const RESPAWN_BACKOFF: Duration = Duration::from_secs(2);
const ICON_NAME: &str = "edit-paste";

const COPY_ACTION_LABEL: &str = "Copy";
const REMOVE_ACTION_LABEL: &str = "Remove";
const SUPPORTED_ACTION_LABELS: &[&str] = &[COPY_ACTION_LABEL, REMOVE_ACTION_LABEL];

pub struct Clipboard {
    history: Arc<RwLock<VecDeque<String>>>,
}

impl Capability for Clipboard {
    fn id(&self) -> &'static str {
        "clipboard"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "Clipboard".into(),
            description: "Browse and reuse clipboard history.".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

impl QueryHandler for Clipboard {
    fn query(&self, input: &str) -> Vec<Item> {
        let q = input.trim().to_lowercase();
        let entries = self.history.read().unwrap();

        entries
            .iter()
            .filter(|e| q.is_empty() || e.to_lowercase().contains(&q))
            .map(|e| build_item(e))
            .collect()
    }

    fn run(&self, action: Action) -> ActionOutcome {
        let Some(text) = action.params.into_iter().next() else {
            error!("clipboard: action with no payload");
            return ActionOutcome::Hide;
        };

        match action.label.as_str() {
            COPY_ACTION_LABEL => self.copy_to_clipboard(&text),
            REMOVE_ACTION_LABEL => self.remove_entry(&text),
            other => {
                error!("clipboard: unknown action label: {other}");
            },
        };

        ActionOutcome::Hide
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

    fn copy_to_clipboard(&self, text: &str) {
        let opts = CopyOptions::new();
        match opts.copy(
            CopySource::Bytes(text.as_bytes().into()),
            CopyMimeType::Autodetect,
        ) {
            Ok(()) => debug!("clipboard: copy succeeded"),
            Err(e) => error!("clipboard: copy failed: {e}"),
        }
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
        match run_wl_paste_watch(&history) {
            Ok(()) => debug!("clipboard: wl-paste --watch exited cleanly; respawning"),
            Err(e) => error!("clipboard: wl-paste watcher error: {e}; respawning"),
        }
        thread::sleep(RESPAWN_BACKOFF);
    }
}

fn run_wl_paste_watch(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
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
            Ok(0) => return Ok(()),
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
                return Err(e);
            },
        }
    }
}

fn push_entry(history: &RwLock<VecDeque<String>>, text: String) {
    let mut g = history.write().unwrap();
    g.retain(|e| *e != text);
    g.push_front(text);
    while g.len() > HISTORY_LIMIT {
        g.pop_back();
    }
}

fn build_item(text: &str) -> Item {
    Item {
        title: text.to_owned(),
        icon: Some(IconRef::Name(ICON_NAME.into())),
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
}
