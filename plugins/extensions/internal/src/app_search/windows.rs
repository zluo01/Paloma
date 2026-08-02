use std::{
    any::Any,
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use log::{debug, error, warn};
use notify::RecursiveMode;
use paloma_extension_protocol::v1::CapabilityIcon;
use windows::{
    ApplicationModel::{PackageCatalog, PackageInstallingEventArgs, PackageUninstallingEventArgs},
    Foundation::TypedEventHandler,
    Win32::{
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, IBindCtx},
        UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_CommonPrograms, FOLDERID_Programs,
            IEnumShellItems, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem,
            SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
        },
    },
    core::PWSTR,
};

use super::{AppEntry, AppSearchBackend, backend::watch_dirs};

pub(super) struct Platform;

impl AppSearchBackend for Platform {
    fn load() -> Vec<AppEntry> {
        // Call COM init explicitly as COM access is per thread,
        // and required for any Shell calls
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        scan_apps_folder().unwrap_or_else(|e| {
            error!("AppsFolder enumeration failed: {e}");
            Vec::new()
        })
    }

    fn watch_paths() -> Vec<PathBuf> {
        [&FOLDERID_CommonPrograms, &FOLDERID_Programs]
            .into_iter()
            .filter_map(crate::file_search::known_folder_path)
            .collect()
    }

    fn is_app_file(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk") || ext.eq_ignore_ascii_case("url"))
    }

    fn launch(params: &[String]) {
        let Some(app_id) = params.first() else {
            error!("empty params, nothing to launch");
            return;
        };

        let target = format!("shell:AppsFolder\\{app_id}");
        match open::that_detached(&target) {
            Ok(()) => debug!("launched {target}"),
            Err(err) => error!("failed to launch {target}: {err}"),
        }
    }

    // Need to watch two places:
    //  - Start Menu for unpackaged installed (i.e. MSI)
    //  - PackageCatalog for packaged installed (UWP)
    fn watch(
        trigger: impl Fn() + Send + Sync + 'static,
    ) -> notify::Result<Box<dyn Any + Send + Sync>> {
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(trigger);
        let debouncer = {
            let trigger = Arc::clone(&trigger);
            watch_dirs(
                Self::watch_paths(),
                RecursiveMode::Recursive,
                Self::is_app_file,
                move || trigger(),
            )?
        };
        match package_watch(trigger) {
            Ok(packages) => Ok(Box::new((debouncer, packages))),
            Err(e) => {
                warn!("package change detection unavailable: {e}");
                Ok(Box::new(debouncer))
            },
        }
    }
}

fn scan_apps_folder() -> windows::core::Result<Vec<AppEntry>> {
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }?;
    let enumerator: IEnumShellItems =
        unsafe { folder.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems) }?;

    let mut entries = Vec::new();
    loop {
        let mut items: [Option<IShellItem>; 1] = [None];
        let mut fetched = 0;
        let hr = unsafe { enumerator.Next(&mut items, Some(&mut fetched)) };
        if hr.is_err() || fetched == 0 {
            break;
        }
        let Some(item) = items[0].take() else { break };

        let Ok(name) = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }.map(take_string) else {
            continue;
        };
        let Ok(app_id) =
            unsafe { item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING) }.map(take_string)
        else {
            continue;
        };
        if name.is_empty() || app_id.is_empty() {
            continue;
        }

        let exec_interest = match_token(&app_id).filter(|interest| *interest != name);
        let icon = Some(CapabilityIcon::path(format!("shell:AppsFolder\\{app_id}")));
        entries.push(AppEntry {
            name,
            generic_name: None,
            keywords: Vec::new(),
            exec: vec![app_id],
            exec_interest,
            icon,
        });
    }
    Ok(entries)
}

struct PackageWatch {
    catalog: PackageCatalog,
    installation_event_token: i64,
    uninstallation_event_token: i64,
}

unsafe impl Send for PackageWatch {}
unsafe impl Sync for PackageWatch {}

impl Drop for PackageWatch {
    fn drop(&mut self) {
        let _ = self
            .catalog
            .RemovePackageInstalling(self.installation_event_token);
        let _ = self
            .catalog
            .RemovePackageUninstalling(self.uninstallation_event_token);
    }
}

// Subscribe to package install and uninstall events
fn package_watch(trigger: Arc<dyn Fn() + Send + Sync>) -> windows::core::Result<PackageWatch> {
    const TRAILING_EDGE: Duration = Duration::from_millis(500);

    let catalog = PackageCatalog::OpenForCurrentUser()?;
    let (tx, rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("paloma-appsearch-packages".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(TRAILING_EDGE).is_ok() {}
                trigger();
            }
        })
        .expect("spawn package watch thread");

    let installation_event_token = {
        let tx = tx.clone();
        catalog.PackageInstalling(&TypedEventHandler::<
            PackageCatalog,
            PackageInstallingEventArgs,
        >::new(move |_, _| {
            let _ = tx.send(());
            Ok(())
        }))?
    };
    let uninstallation_event_token = catalog.PackageUninstalling(&TypedEventHandler::<
        PackageCatalog,
        PackageUninstallingEventArgs,
    >::new(move |_, _| {
        let _ = tx.send(());
        Ok(())
    }))?;

    Ok(PackageWatch {
        catalog,
        installation_event_token,
        uninstallation_event_token,
    })
}

// get the string from ptr and manually free the shell allocated buffer
fn take_string(pw: PWSTR) -> String {
    let s = unsafe { pw.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(pw.as_ptr() as *const c_void)) };
    s
}

fn match_token(app_id: &str) -> Option<String> {
    if let Some((family, _)) = app_id.split_once('!') {
        return Some(family.to_string());
    }

    let bytes = app_id.as_bytes();
    let path = app_id
        .strip_prefix('{')
        .and_then(|rest| rest.split_once("}\\"))
        .map(|(_, path)| path)
        .or_else(|| (bytes.len() > 2 && bytes[1] == b':' && bytes[2] == b'\\').then_some(app_id));

    let stem = Path::new(path?).file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_watch_subscribes_without_package_identity() {
        assert!(package_watch(Arc::new(|| {})).is_ok());
    }

    #[test]
    fn watches_both_start_menu_roots() {
        let paths = Platform::watch_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|p| p.ends_with("Programs")));
    }

    #[test]
    fn only_shortcut_files_are_app_files() {
        assert!(Platform::is_app_file(Path::new(r"C:\x\App Name.lnk")));
        assert!(Platform::is_app_file(Path::new(r"C:\x\APP.LNK")));
        assert!(Platform::is_app_file(Path::new(r"C:\x\Site.url")));
        assert!(!Platform::is_app_file(Path::new(r"C:\x\Vendor Folder")));
        assert!(!Platform::is_app_file(Path::new(r"C:\x\desktop.ini")));
        assert!(!Platform::is_app_file(Path::new(r"C:\x\tool.exe")));
    }

    #[test]
    fn match_token_covers_the_six_appid_shapes() {
        assert_eq!(
            match_token("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App").as_deref(),
            Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe")
        );
        assert_eq!(
            match_token(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe").as_deref(),
            Some("cmd")
        );
        assert_eq!(
            match_token(r"C:\Program Files\7-Zip\7zFM.exe").as_deref(),
            Some("7zFM")
        );
        assert_eq!(match_token("Chrome"), None);
        assert_eq!(match_token("308046B0AF4A39CB;PrivateBrowsingAUMID"), None);
        assert_eq!(
            match_token("Microsoft.AutoGenerated.{ED1B95B9-0000-0000-0000-000000000000}"),
            None
        );
    }
}
