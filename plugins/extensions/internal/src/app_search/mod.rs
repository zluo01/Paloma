#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Platform;

#[cfg(target_os = "macos")]
mod macos;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use log::{debug, info, warn};
#[cfg(target_os = "macos")]
use macos::Platform;
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use paloma_extension_base::{Capability, SearchHandler};
use paloma_extension_protocol::v1::{
    Action, CapabilityIcon, Hide, Item, run_action_response::Behavior,
};

/// Contract each platform backend implements on its `Platform` struct.
/// Porting to a new OS means adding a module above and implementing this.
trait AppSearchBackend {
    /// Discover installed applications.
    fn load() -> Vec<AppEntry>;
    /// Directories to watch (non-recursively) for application
    /// install/uninstall changes.
    fn watch_paths() -> Vec<PathBuf>;
    /// Whether a filesystem event path is relevant to the application
    /// index and should trigger a rescan.
    fn is_app_file(path: &Path) -> bool;
    /// Launch the application referenced by an action's params.
    fn launch(params: &[String]);
}

const NAME_WEIGHT: u32 = 10_000;
const GENERIC_NAME_WEIGHT: u32 = 7_000;
const KEYWORD_WEIGHT: u32 = 5_000;
const EXEC_WEIGHT: u32 = 3_000;

struct AppEntry {
    name: String,
    generic_name: Option<String>,
    keywords: Vec<String>,
    /// Params passed to `AppSearchBackend::launch`; not used for matching.
    exec: Vec<String>,
    /// Extra low-weight haystack for matching (e.g. the binary or bundle
    /// name) — kept separate from `exec` so shared path prefixes like
    /// "/Applications" don't make every entry match.
    exec_interest: Option<String>,
    icon: Option<CapabilityIcon>,
}

impl AppEntry {
    fn to_item(&self) -> Item {
        Item {
            title: self.name.clone(),
            subtitle: self.generic_name.clone(),
            icon: self.icon.clone(),
            actions: vec![Action {
                label: "Open".to_string(),
                params: self.exec.clone(),
                primary: true,
            }],
        }
    }
}

pub struct AppSearch {
    entries: Arc<RwLock<Vec<AppEntry>>>,
    _watcher: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl Capability for AppSearch {
    fn id(&self) -> &str {
        "App Search"
    }

    fn description(&self) -> &str {
        "Launch installed applications."
    }

    fn search_handler(&self) -> Option<&dyn SearchHandler> {
        Some(self)
    }
}

impl SearchHandler for AppSearch {
    fn search(&self, input: &str) -> Vec<Item> {
        let pattern = Pattern::parse(input.trim(), CaseMatching::Ignore, Normalization::Smart);
        if pattern.atoms.is_empty() {
            return Vec::new();
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let entries = self.entries.read().unwrap();

        let mut ranked: Vec<_> = entries
            .iter()
            .filter_map(|app| {
                score_app(&pattern, app, &mut matcher, &mut buf).map(|score| (score, app))
            })
            .collect();

        ranked.sort_by(|(left_score, left), (right_score, right)| {
            right_score.cmp(left_score).then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
        });

        ranked.into_iter().map(|(_, app)| app.to_item()).collect()
    }

    fn run_search_action(&self, action: Action) -> Behavior {
        Platform::launch(&action.params);
        Behavior::Hide(Hide {})
    }
}

impl AppSearch {
    pub fn new() -> notify::Result<Self> {
        let entries: Arc<RwLock<Vec<AppEntry>>> = Arc::new(RwLock::new(Vec::new()));
        let entries_for_watcher = Arc::clone(&entries);

        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            None,
            move |result: DebounceEventResult| {
                let Ok(events) = result else { return };
                let should_load = events.iter().any(|e| {
                    matches!(
                        e.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) && e.paths.iter().any(|p| Platform::is_app_file(p))
                });
                if !should_load {
                    return;
                }
                let fresh = Platform::load();
                if let Ok(mut guard) = entries_for_watcher.write() {
                    *guard = fresh;
                }
            },
        )?;

        for path in Platform::watch_paths() {
            if !path.is_dir() {
                debug!("skipping missing watch path {path:?}");
                continue;
            }
            match debouncer.watch(&path, RecursiveMode::NonRecursive) {
                Ok(_) => info!("watching {path:?}"),
                Err(e) => warn!("failed to watch {path:?}: {e}"),
            }
        }

        {
            let entries = Arc::clone(&entries);
            thread::Builder::new()
                .name("paloma-appsearch-index".into())
                .spawn(move || {
                    let loaded = Platform::load();
                    info!("indexed {} applications", loaded.len());
                    *entries.write().unwrap() = loaded;
                })
                .expect("spawn app index thread");
        }

        Ok(Self {
            entries,
            _watcher: debouncer,
        })
    }
}

fn score_app(
    pattern: &Pattern,
    app: &AppEntry,
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
) -> Option<u32> {
    let fields = std::iter::once((app.name.as_str(), NAME_WEIGHT))
        .chain(
            app.generic_name
                .as_deref()
                .map(|s| (s, GENERIC_NAME_WEIGHT)),
        )
        .chain(
            app.keywords
                .iter()
                .filter(|kw| !kw.is_empty())
                .map(|kw| (kw.as_str(), KEYWORD_WEIGHT)),
        )
        .chain(app.exec_interest.as_deref().map(|s| (s, EXEC_WEIGHT)));

    let fields: Vec<_> = fields.collect();
    let mut total = 0;

    for atom in &pattern.atoms {
        let best = fields
            .iter()
            .filter_map(|(field, weight)| {
                let haystack = Utf32Str::new(field, buf);
                atom.score(haystack, matcher)
                    .map(|score| u32::from(score) + weight)
            })
            .max()?;
        total += best;
    }

    Some(total)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;

    static FIREFOX: LazyLock<AppEntry> = LazyLock::new(|| {
        app(
            "Firefox",
            Some("Web Browser"),
            &["web", "browser", "internet"],
            &["firefox"],
        )
    });

    static FIREWALL: LazyLock<AppEntry> = LazyLock::new(|| {
        app(
            "Firewall",
            None,
            &["firewall", "network", "security", "iptables", "netfilter"],
            &["/usr/bin/firewall-config"],
        )
    });

    static FILELIGHT: LazyLock<AppEntry> = LazyLock::new(|| {
        app(
            "Filelight",
            Some("Disk Usage Statistics"),
            &["disk", "drive", "space", "storage", "usage"],
            &["filelight"],
        )
    });

    static KFIND: LazyLock<AppEntry> = LazyLock::new(|| {
        app(
            "KFind",
            Some("Find Files/Folders"),
            &["search", "file search", "search tool", "finder"],
            &["kfind"],
        )
    });

    static VSCODE: LazyLock<AppEntry> = LazyLock::new(|| {
        app(
            "Visual Studio Code",
            Some("Text Editor"),
            &["vscode"],
            &["/usr/share/code/code"],
        )
    });

    fn app(name: &str, generic_name: Option<&str>, keywords: &[&str], exec: &[&str]) -> AppEntry {
        AppEntry {
            name: name.to_string(),
            generic_name: generic_name.map(str::to_string),
            keywords: keywords.iter().map(|kw| kw.to_string()).collect(),
            exec: exec.iter().map(|arg| arg.to_string()).collect(),
            exec_interest: exec.first().map(|arg| arg.to_string()),
            icon: None,
        }
    }

    fn score(query: &str, app: &AppEntry) -> Option<u32> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        score_app(&pattern, app, &mut matcher, &mut buf)
    }

    #[test]
    fn unrelated_apps_do_not_match() {
        assert!(score("fire", &FIREFOX).is_some());
        assert!(score("fire", &FIREWALL).is_some());
        assert_eq!(score("fire", &FILELIGHT), None);
        assert_eq!(score("fire", &KFIND), None);
    }

    #[test]
    fn every_query_word_must_match() {
        assert!(score("fire web", &FIREFOX).is_some());
        assert_eq!(score("fire web", &FIREWALL), None);
    }

    #[test]
    fn names_rank_ahead_of_generic_names_and_keywords() {
        let browser = app("Browser", Some("Firefox helper"), &["fire"], &["browser"]);

        assert!(score("fire", &FIREFOX) > score("fire", &browser));
    }

    #[test]
    fn acronyms_and_keyword_prefixes_match() {
        assert!(score("vsc", &VSCODE).is_some());
    }
}
