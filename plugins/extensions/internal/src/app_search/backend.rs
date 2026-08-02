use std::{
    any::Any,
    path::{Path, PathBuf},
    time::Duration,
};

use log::{debug, info, warn};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use paloma_extension_protocol::v1::{Action, CapabilityIcon, Item};

pub(super) trait AppSearchBackend {
    fn load() -> Vec<AppEntry>;
    fn watch_paths() -> Vec<PathBuf>;
    fn is_app_file(path: &Path) -> bool;
    fn launch(params: &[String]);
    fn watch(
        trigger: impl Fn() + Send + Sync + 'static,
    ) -> notify::Result<Box<dyn Any + Send + Sync>> {
        watch_dirs(
            Self::watch_paths(),
            RecursiveMode::NonRecursive,
            Self::is_app_file,
            trigger,
        )
    }
}

pub(super) struct AppEntry {
    pub(super) name: String,
    pub(super) generic_name: Option<String>,
    pub(super) keywords: Vec<String>,
    /// Params passed to `AppSearchBackend::launch`; not used for matching.
    pub(super) exec: Vec<String>,
    /// Extra low-weight haystack for matching (e.g. the binary or bundle
    /// name) — kept separate from `exec` so shared path prefixes like
    /// "/Applications" don't make every entry match.
    pub(super) exec_interest: Option<String>,
    pub(super) icon: Option<CapabilityIcon>,
}

impl AppEntry {
    pub(super) fn to_item(&self) -> Item {
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

/// Debounced directory watch calling `trigger` on relevant filesystem
/// events — the default change detection for every backend.
pub(super) fn watch_dirs(
    paths: Vec<PathBuf>,
    mode: RecursiveMode,
    is_app_file: fn(&Path) -> bool,
    trigger: impl Fn() + Send + Sync + 'static,
) -> notify::Result<Box<dyn Any + Send + Sync>> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let relevant = events.iter().any(|e| {
                matches!(
                    e.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) && e.paths.iter().any(|p| is_app_file(p))
            });
            if relevant {
                trigger();
            }
        },
    )?;

    for path in paths {
        if !path.is_dir() {
            debug!("skipping missing watch path {path:?}");
            continue;
        }
        match debouncer.watch(&path, mode) {
            Ok(_) => info!("watching {path:?}"),
            Err(e) => warn!("failed to watch {path:?}: {e}"),
        }
    }

    Ok(Box::new(debouncer))
}
