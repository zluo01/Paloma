use windows::{
    Win32::{
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, IBindCtx,
        },
        UI::Shell::{
            FOF_NO_UI, FOFX_ADDUNDORECORD, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
            IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName,
        },
    },
    core::HSTRING,
};

use super::DeleteError;

pub(super) async fn trash(paths: &[String]) -> Result<Vec<(String, String)>, DeleteError> {
    let paths = paths.to_vec();
    // delete is blocking, hence move to a blocking thread
    let failures = tokio::task::spawn_blocking(move || {
        // Call COM init explicitly as COM access is per thread,
        // and required for any Shell calls
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut failures = Vec::new();
        for path in paths {
            if let Err(message) = trash_one(&path) {
                failures.push((path, message));
            }
        }
        failures
    })
    .await?;
    Ok(failures)
}

fn trash_one(path: &str) -> Result<(), String> {
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None::<&IBindCtx>) }
            .map_err(|e| e.to_string())?;
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(|e| e.to_string())?;
    unsafe {
        operation
            .SetOperationFlags(FOF_NO_UI | FOFX_RECYCLEONDELETE | FOFX_ADDUNDORECORD)
            .map_err(|e| e.to_string())?;
        operation
            .DeleteItem(&item, None::<&IFileOperationProgressSink>)
            .map_err(|e| e.to_string())?;
        operation.PerformOperations().map_err(|e| e.to_string())?;
        if operation
            .GetAnyOperationsAborted()
            .map_err(|e| e.to_string())?
            .as_bool()
        {
            return Err("the shell aborted the operation".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, fs};

    use windows::{
        Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{
                BHID_EnumItems, FOLDERID_RecycleBinFolder, IEnumShellItems, KF_FLAG_DEFAULT,
                SHGetKnownFolderItem, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
            },
        },
        core::PWSTR,
    };

    use super::*;

    fn unique_name(suffix: &str) -> String {
        format!("paloma-shell-test-{}-{suffix}", uuid::Uuid::now_v7())
    }

    // get the string from ptr and manually free the shell allocated buffer
    fn take_string(pw: PWSTR) -> String {
        let s = unsafe { pw.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(pw.as_ptr() as *const c_void)) };
        s
    }

    // A recycled item displays as its original path, hence match
    // its file name against the stem.
    fn find_in_recycle_bin(stem: &str) -> Option<IShellItem> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let bin: IShellItem =
            unsafe { SHGetKnownFolderItem(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None) }
                .ok()?;
        let enumerator: IEnumShellItems =
            unsafe { bin.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems) }.ok()?;
        loop {
            let mut items: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0;
            let hr = unsafe { enumerator.Next(&mut items, Some(&mut fetched)) };
            if hr.is_err() || fetched == 0 {
                return None;
            }
            let item = items[0].take()?;
            let name = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }
                .map(take_string)
                .unwrap_or_default();
            let matched = std::path::Path::new(&name)
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(stem));
            if matched {
                return Some(item);
            }
        }
    }

    // Permanently delete a recycled item, so the bin is left clean.
    fn purge(item: &IShellItem) {
        unsafe {
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();
            operation.SetOperationFlags(FOF_NO_UI).unwrap();
            operation
                .DeleteItem(item, None::<&IFileOperationProgressSink>)
                .unwrap();
            operation.PerformOperations().unwrap();
        }
    }

    #[tokio::test]
    async fn trash_moves_file_and_keeps_content_restorable() {
        let root = tempfile::tempdir().unwrap();
        let stem = unique_name("delete");
        let delete = root.path().join(format!("{stem}.txt"));
        fs::write(&delete, "delete").unwrap();

        let failures = trash(&[delete.to_string_lossy().into_owned()])
            .await
            .unwrap();

        assert!(failures.is_empty(), "got: {failures:?}");
        assert!(!delete.exists());
        let landed = find_in_recycle_bin(&stem).expect("recycled item not found");
        let backing = unsafe { landed.GetDisplayName(SIGDN_FILESYSPATH) }
            .map(take_string)
            .unwrap();
        assert_eq!(fs::read_to_string(&backing).unwrap(), "delete");
        purge(&landed);
    }

    #[tokio::test]
    async fn trash_reports_failure_per_entry_and_continues() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.txt");
        let stem = unique_name("ok");
        let deletable = root.path().join(format!("{stem}.txt"));
        fs::write(&deletable, "ok").unwrap();

        let failures = trash(&[
            missing.to_string_lossy().into_owned(),
            deletable.to_string_lossy().into_owned(),
        ])
        .await
        .unwrap();

        let (target, message) = &failures[0];
        assert_eq!(failures.len(), 1, "got: {failures:?}");
        assert_eq!(target, &missing.to_string_lossy());
        assert!(!message.is_empty());
        // The failure did not stop the rest of the batch.
        assert!(!deletable.exists());
        let landed = find_in_recycle_bin(&stem).expect("recycled item not found");
        purge(&landed);
    }
}
