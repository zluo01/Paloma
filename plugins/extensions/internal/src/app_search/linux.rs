use std::{
    collections::HashSet,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use freedesktop_desktop_entry::{self as fde, DesktopEntry};
use log::{debug, error};
use scry_extension_protocol::v1::{CapabilityIcon, capability_icon};

use super::{AppEntry, AppSearchBackend};

pub(super) struct Platform;

impl AppSearchBackend for Platform {
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

    fn watch_paths() -> Vec<PathBuf> {
        fde::default_paths().collect()
    }

    fn is_app_file(path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "desktop")
    }

    fn launch(params: &[String]) {
        let Some((program, args)) = params.split_first() else {
            error!("empty argv, nothing to launch");
            return;
        };

        let result = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();

        match result {
            Ok(_child) => debug!("launched {program}"),
            Err(err) => error!("failed to launch {program}: {err}"),
        }
    }
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
    let exec_interest = exec
        .iter()
        .find(|tok| !is_interpreter(tok))
        .or_else(|| exec.first())
        .cloned();
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
    let icon = de.icon().map(|s| CapabilityIcon {
        icon: Some(capability_icon::Icon::Name(s.to_owned())),
    });
    Some(AppEntry {
        name,
        generic_name,
        keywords,
        exec,
        exec_interest,
        icon,
    })
}

/// Interpreter / wrapper binaries we don't want to count as the "app name"
/// for search interests. Pattern from Albert.
fn is_interpreter(tok: &str) -> bool {
    let last = Path::new(tok)
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
    use super::*;

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
}
