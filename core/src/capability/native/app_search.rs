use std::{
    collections::HashSet,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    time::Duration,
};

use freedesktop_desktop_entry::{self as fde, DesktopEntry};
use log::{debug, error, info, warn};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::capability::{
    Action, ActionOutcome, Capability, CapabilityMeta, IconRef, Item, QueryHandler,
};

const NAME_WEIGHT: u32 = 10_000;
const GENERIC_NAME_WEIGHT: u32 = 7_000;
const KEYWORD_WEIGHT: u32 = 5_000;
const EXEC_WEIGHT: u32 = 3_000;

struct AppEntry {
    name: String,
    generic_name: Option<String>,
    keywords: Vec<String>,
    exec: Vec<String>,
    icon: Option<String>,
}

impl AppEntry {
    fn to_item(&self) -> Item {
        Item {
            title: self.name.clone(),
            subtitle: self.generic_name.clone(),
            icon: self.icon.clone().map(IconRef::Name),
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
    fn id(&self) -> &'static str {
        "app_search"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "App Search".to_string(),
            description: "Launch installed applications.".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

impl QueryHandler for AppSearch {
    fn query(&self, input: &str) -> Vec<Item> {
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

    fn run(&self, action: Action) -> ActionOutcome {
        let Some((program, args)) = action.params.split_first() else {
            error!("app_search: empty argv, nothing to launch");
            return ActionOutcome::Hide;
        };

        let result = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();

        match result {
            Ok(_child) => debug!("app_search: launched {program}"),
            Err(err) => error!("app_search: failed to launch {program}: {err}"),
        }

        ActionOutcome::Hide
    }
}

impl AppSearch {
    pub fn new() -> notify::Result<Self> {
        let entries = Arc::new(RwLock::new(load()));
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
                    ) && e
                        .paths
                        .iter()
                        .any(|p| p.extension().is_some_and(|ext| ext == "desktop"))
                });
                if !should_load {
                    return;
                }
                let fresh = load();
                if let Ok(mut guard) = entries_for_watcher.write() {
                    *guard = fresh;
                }
            },
        )?;

        for path in fde::default_paths() {
            if !path.is_dir() {
                debug!("app_search: skipping missing watch path {path:?}");
                continue;
            }
            match debouncer.watch(&path, RecursiveMode::NonRecursive) {
                Ok(_) => info!("app_search: watching {path:?}"),
                Err(e) => warn!("app_search: failed to watch {path:?}: {e}"),
            }
        }

        Ok(Self {
            entries,
            _watcher: debouncer,
        })
    }
}

fn load() -> Vec<AppEntry> {
    let locales = fde::get_languages_from_env();
    let current_desktop = fde::current_desktop();
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries = Vec::new();

    for path in fde::Iter::new(fde::default_paths()) {
        let Ok(de) = DesktopEntry::from_path(path, Some(&locales)) else {
            continue;
        };
        if !keep(&de, current_desktop.as_deref(), &mut seen) {
            continue;
        }
        let Some(app) = decode(&de, &locales) else {
            continue;
        };
        entries.push(app);
    }

    entries
}

fn score_app(
    pattern: &Pattern,
    app: &AppEntry,
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
) -> Option<u32> {
    let exec_interest = app
        .exec
        .iter()
        .find(|tok| !is_interpreter(tok))
        .or_else(|| app.exec.first());

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
        .chain(exec_interest.map(|s| (s.as_str(), EXEC_WEIGHT)));

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

fn keep(de: &DesktopEntry, current_desktop: Option<&[String]>, seen: &mut HashSet<String>) -> bool {
    let appid = match de.flatpak() {
        Some(base) => format!("{}.{}", base, de.appid),
        None => de.appid.to_string(),
    };
    if seen.contains(&appid) {
        return false;
    }

    let exec_first = de.exec().and_then(|e| e.split_ascii_whitespace().next());
    if matches!(exec_first, None | Some("false")) {
        return false;
    }

    if let (Some(not_show), Some(current)) = (de.not_show_in(), current_desktop)
        && not_show
            .iter()
            .any(|d| current.iter().any(|c| c.eq_ignore_ascii_case(d)))
    {
        return false;
    }

    if let Some(only_show) = de.only_show_in() {
        if let Some(current) = current_desktop
            && !only_show
                .iter()
                .any(|d| current.iter().any(|c| c.eq_ignore_ascii_case(d)))
        {
            return false;
        }
    } else if de.no_display() {
        return false;
    }

    seen.insert(appid);
    true
}

fn decode(de: &DesktopEntry, locales: &[String]) -> Option<AppEntry> {
    let name = de.name(locales)?.to_string();
    let exec = de.parse_exec().ok().filter(|v| !v.is_empty())?;
    let generic_name = de.generic_name(locales).map(|s| s.to_string());
    let keywords = de
        .keywords(locales)
        .map(|kws| {
            kws.iter()
                .filter(|kw| !kw.trim().is_empty())
                .map(|kw| kw.to_string())
                .collect()
        })
        .unwrap_or_default();
    let icon = de.icon().map(str::to_owned);
    Some(AppEntry {
        name,
        generic_name,
        keywords,
        exec,
        icon,
    })
}

/// Interpreter / wrapper binaries we don't want to count as the "app name"
/// for search interests. Pattern from Albert.
fn is_interpreter(tok: &str) -> bool {
    let last = std::path::Path::new(tok)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(tok);
    matches!(
        last,
        "bash"
            | "dbus-send"
            | "env"
            | "flatpak"
            | "java"
            | "perl"
            | "python"
            | "python2"
            | "python3"
            | "ruby"
            | "sh"
    )
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
    fn decode_filters_empty_keywords() {
        let mut entry = DesktopEntry::from_appid("firefox.desktop".to_string());
        entry.add_desktop_entry("Name".to_string(), "Firefox".to_string());
        entry.add_desktop_entry("Exec".to_string(), "firefox".to_string());
        entry.add_desktop_entry(
            "Keywords".to_string(),
            "web;browser;internet;; ".to_string(),
        );

        let app = decode(&entry, &[]).expect("test desktop entry should decode");

        assert_eq!(app.keywords, ["web", "browser", "internet"]);
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
