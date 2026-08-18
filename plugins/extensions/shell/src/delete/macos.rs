use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::DeleteError;

pub(super) async fn trash(paths: &[String]) -> Result<Vec<(String, String)>, DeleteError> {
    let paths = paths.to_vec();
    // delete is blocking, hence move to a blocking thread
    let failures = tokio::task::spawn_blocking(move || {
        let manager = NSFileManager::defaultManager();
        let mut failures = Vec::new();
        for path in paths {
            if let Err(message) = trash_one(&manager, &path) {
                failures.push((path, message));
            }
        }
        failures
    })
    .await
    .expect("trash task panicked");
    Ok(failures)
}

fn trash_one(manager: &NSFileManager, path: &str) -> Result<(), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    match manager.trashItemAtURL_resultingItemURL_error(&url, None) {
        Ok(()) => Ok(()),
        Err(error) => Err(error.localizedDescription().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
    };

    use super::*;

    fn unique_name(suffix: &str) -> String {
        format!("paloma-shell-test-{}-{suffix}", uuid::Uuid::now_v7())
    }

    fn trashed_location(name: &str) -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap())
            .join(".Trash")
            .join(name)
    }

    #[tokio::test]
    async fn trash_moves_file_and_keeps_content_restorable() {
        let root = tempfile::tempdir().unwrap();
        let name = unique_name("delete.txt");
        let delete = root.path().join(&name);
        fs::write(&delete, "delete").unwrap();

        let failures = trash(&[delete.to_string_lossy().into_owned()])
            .await
            .unwrap();

        assert!(failures.is_empty(), "got: {failures:?}");
        assert!(!delete.exists());
        let landed = trashed_location(&name);
        assert_eq!(fs::read_to_string(&landed).unwrap(), "delete");
        fs::remove_file(&landed).unwrap();
    }

    #[tokio::test]
    async fn trash_keeps_symlink_target_untouched() {
        let root = tempfile::tempdir().unwrap();
        let name = unique_name("link");
        let target = root.path().join("target.txt");
        let link = root.path().join(&name);
        fs::write(&target, "kept").unwrap();
        symlink(&target, &link).unwrap();

        let failures = trash(&[link.to_string_lossy().into_owned()]).await.unwrap();

        assert!(failures.is_empty(), "got: {failures:?}");
        assert!(!link.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "kept");
        fs::remove_file(trashed_location(&name)).unwrap();
    }

    #[tokio::test]
    async fn trash_reports_failure_per_entry_and_continues() {
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        let blocked = locked.join("delete.txt");
        fs::create_dir(&locked).unwrap();
        fs::write(&blocked, "delete").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let name = unique_name("ok.txt");
        let deletable = root.path().join(&name);
        fs::write(&deletable, "ok").unwrap();

        let failures = trash(&[
            blocked.to_string_lossy().into_owned(),
            deletable.to_string_lossy().into_owned(),
        ])
        .await
        .unwrap();

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let (target, message) = &failures[0];
        assert_eq!(failures.len(), 1, "got: {failures:?}");
        assert_eq!(target, &blocked.to_string_lossy());
        assert!(message.contains("delete"), "got: {message}");
        assert!(blocked.exists());
        // The failure did not stop the rest of the batch.
        assert!(!deletable.exists());
        fs::remove_file(trashed_location(&name)).unwrap();
    }
}
