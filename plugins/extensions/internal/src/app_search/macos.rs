use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use log::{debug, error};
use paloma_extension_protocol::v1::CapabilityIcon;

use super::{AppEntry, AppSearchBackend};

pub(super) struct Platform;

impl AppSearchBackend for Platform {
    fn load() -> Vec<AppEntry> {
        // Dedupes by bundle identifier in root order, so the system-wide
        // copy wins over a per-user copy in ~/Applications.
        let mut seen: HashSet<String> = HashSet::new();
        let mut entries = Vec::new();
        for root in Self::watch_paths() {
            scan_root(&root, &mut seen, &mut entries);
        }
        entries
    }

    // /System/Applications/Utilities is covered by the one-level descent
    // from /System/Applications.
    fn watch_paths() -> Vec<PathBuf> {
        let mut paths = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(home) = std::env::home_dir() {
            // May not exist on fresh accounts; create the standard folder
            // up front, else the watcher would skip it forever and apps
            // installed there later would never appear.
            let user_apps = home.join("Applications");
            if let Err(e) = std::fs::create_dir_all(&user_apps) {
                debug!("could not create {user_apps:?}: {e}");
            }
            paths.push(user_apps);
        }
        paths
    }

    // The watch is non-recursive, so every event names a direct child of a
    // root — a bundle or a vendor folder, either way worth a (debounced,
    // cheap) rescan. Only Finder metadata churn like .DS_Store is ignored,
    // else browsing /Applications would trigger rescans.
    fn is_app_file(path: &Path) -> bool {
        !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
    }

    fn launch(params: &[String]) {
        let Some(bundle) = params.first() else {
            error!("empty params, nothing to launch");
            return;
        };

        match open::that_detached(bundle) {
            Ok(()) => debug!("launched {bundle}"),
            Err(err) => error!("failed to launch {bundle}: {err}"),
        }
    }
}

fn is_bundle(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "app")
}

fn dirs_in(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
}

// Bundles at the root, plus one level of vendor subfolders so apps under
// "/Applications/Adobe Photoshop/" are still found.
fn scan_root(root: &Path, seen: &mut HashSet<String>, entries: &mut Vec<AppEntry>) {
    for child in dirs_in(root) {
        if is_bundle(&child) {
            push_bundle(&child, seen, entries);
        } else {
            for nested in dirs_in(&child).filter(|path| is_bundle(path)) {
                push_bundle(&nested, seen, entries);
            }
        }
    }
}

fn push_bundle(bundle: &Path, seen: &mut HashSet<String>, entries: &mut Vec<AppEntry>) {
    if let Some((key, app)) = decode(bundle)
        && seen.insert(key)
    {
        entries.push(app);
    }
}

// Per-key, so an empty CFBundleDisplayName doesn't suppress the fallback
// to a valid CFBundleName.
fn non_empty(info: Option<&plist::Dictionary>, key: &str) -> Option<String> {
    let value = info?.get(key)?.as_string()?;
    (!value.trim().is_empty()).then(|| value.to_owned())
}

/// Returns the dedupe key (bundle identifier, falling back to the path)
/// and the decoded entry.
fn decode(bundle: &Path) -> Option<(String, AppEntry)> {
    let info_plist = bundle.join("Contents/Info.plist");
    if !info_plist.is_file() {
        return None;
    }

    let info = plist::Value::from_file(&info_plist)
        .ok()
        .and_then(|value| value.into_dictionary());
    let stem = bundle.file_stem()?.to_str()?.to_string();
    let path = bundle.to_str()?.to_string();

    let name = non_empty(info.as_ref(), "CFBundleDisplayName")
        .or_else(|| non_empty(info.as_ref(), "CFBundleName"))
        .unwrap_or_else(|| stem.clone());
    let exec_interest = (name != stem).then_some(stem);
    let key = non_empty(info.as_ref(), "CFBundleIdentifier").unwrap_or_else(|| path.clone());

    let app = AppEntry {
        name,
        generic_name: None,
        keywords: Vec::new(),
        exec: vec![path.clone()],
        exec_interest,
        icon: Some(CapabilityIcon::path(path)),
    };
    Some((key, app))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bundle(root: &Path, dir_name: &str, plist_xml: Option<&str>) -> PathBuf {
        let bundle = root.join(dir_name);
        let contents = bundle.join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        if let Some(xml) = plist_xml {
            std::fs::write(contents.join("Info.plist"), xml).unwrap();
        }
        bundle
    }

    fn info_plist(entries: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>{entries}</dict>
</plist>"#
        )
    }

    #[test]
    fn load_finds_installed_apps() {
        // Every macOS install ships bundles under /System/Applications.
        assert!(!Platform::load().is_empty());
    }

    #[test]
    fn decode_prefers_display_name() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = write_bundle(
            tmp.path(),
            "Code.app",
            Some(&info_plist(
                "<key>CFBundleIdentifier</key><string>com.microsoft.VSCode</string>\
                 <key>CFBundleName</key><string>Code</string>\
                 <key>CFBundleDisplayName</key><string>Visual Studio Code</string>",
            )),
        );

        let (key, app) = decode(&bundle).expect("bundle should decode");

        assert_eq!(key, "com.microsoft.VSCode");
        assert_eq!(app.name, "Visual Studio Code");
        assert_eq!(app.exec, [bundle.to_str().unwrap()]);
        // The bundle stem stays matchable without exposing the full path.
        assert_eq!(app.exec_interest.as_deref(), Some("Code"));
        assert_eq!(
            app.icon,
            Some(CapabilityIcon::path(bundle.to_str().unwrap()))
        );
    }

    #[test]
    fn decode_falls_back_to_file_stem_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = write_bundle(tmp.path(), "Safari.app", Some(&info_plist("")));

        let (key, app) = decode(&bundle).expect("bundle should decode");

        assert_eq!(key, bundle.to_str().unwrap());
        assert_eq!(app.name, "Safari");
        assert_eq!(app.exec_interest, None);
    }

    #[test]
    fn empty_display_name_falls_back_to_bundle_name() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = write_bundle(
            tmp.path(),
            "Foo.app",
            Some(&info_plist(
                "<key>CFBundleDisplayName</key><string></string>\
                 <key>CFBundleName</key><string>Foo Product</string>",
            )),
        );

        let (_, app) = decode(&bundle).expect("bundle should decode");

        assert_eq!(app.name, "Foo Product");
    }

    #[test]
    fn finder_metadata_events_are_ignored() {
        assert!(Platform::is_app_file(Path::new("/Applications/Tool.app")));
        assert!(Platform::is_app_file(Path::new(
            "/Applications/Vendor Folder"
        )));
        assert!(!Platform::is_app_file(Path::new("/Applications/.DS_Store")));
    }

    #[test]
    fn decode_rejects_dirs_without_info_plist() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = write_bundle(tmp.path(), "NotAnApp.app", None);

        assert!(decode(&bundle).is_none());
    }

    #[test]
    fn scan_finds_nested_bundles() {
        let tmp = tempfile::tempdir().unwrap();
        write_bundle(tmp.path(), "Top.app", Some(&info_plist("")));
        let vendor = tmp.path().join("Vendor Folder");
        std::fs::create_dir_all(&vendor).unwrap();
        write_bundle(&vendor, "Nested.app", Some(&info_plist("")));

        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        scan_root(tmp.path(), &mut seen, &mut entries);

        let mut names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["Nested", "Top"]);
    }

    #[test]
    fn bundles_sharing_an_identifier_dedupe_across_roots() {
        let plist = info_plist(
            "<key>CFBundleIdentifier</key><string>com.google.Chrome</string>\
             <key>CFBundleName</key><string>Google Chrome</string>",
        );
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_bundle(system.path(), "Google Chrome.app", Some(&plist));
        write_bundle(user.path(), "Google Chrome.app", Some(&plist));

        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        scan_root(system.path(), &mut seen, &mut entries);
        scan_root(user.path(), &mut seen, &mut entries);

        // The copy in the earlier-scanned (system) root wins.
        assert_eq!(entries.len(), 1);
        assert!(entries[0].exec[0].starts_with(system.path().to_str().unwrap()));
    }
}
