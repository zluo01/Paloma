use std::process::Stdio;

use tokio::process::Command;

use super::DeleteError;

pub(super) async fn trash(paths: &[String]) -> Result<Vec<(String, String)>, DeleteError> {
    if !gio_available().await {
        return Err(DeleteError::GioNotFound);
    }
    let mut failures = Vec::new();
    for path in paths {
        if let Err(message) = gio_trash(path).await {
            failures.push((path.clone(), message));
        }
    }
    Ok(failures)
}

async fn gio_available() -> bool {
    let mut command = Command::new("gio");
    command.arg("version");
    run(command).await.is_ok()
}

async fn gio_trash(path: &str) -> Result<(), String> {
    let mut command = Command::new("gio");
    command.args(["trash", "--", path]);
    run(command).await
}

async fn run(mut command: Command) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to spawn gio: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        Err(format!("gio trash exited with {}", output.status))
    } else {
        Err(stderr.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use super::*;

    fn gio_trash_into(path: &Path, root: &Path) -> Command {
        let mut command = Command::new("gio");
        command
            .args(["trash", "--", path.to_str().unwrap()])
            .env("HOME", root)
            .env("XDG_DATA_HOME", root.join("data"));
        command
    }

    #[tokio::test]
    async fn trash_moves_file_and_keeps_content_restorable() {
        if !gio_available().await {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let delete = root.path().join("delete.txt");
        fs::write(&delete, "delete").unwrap();

        let result = run(gio_trash_into(&delete, root.path())).await;

        assert_eq!(result, Ok(()));
        assert!(!delete.exists());
        assert_eq!(
            fs::read_to_string(root.path().join("data/Trash/files/delete.txt")).unwrap(),
            "delete"
        );
    }

    #[tokio::test]
    async fn trash_reports_raw_gio_error() {
        if !gio_available().await {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        let delete = locked.join("delete.txt");
        fs::create_dir(&locked).unwrap();
        fs::write(&delete, "delete").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();

        let result = run(gio_trash_into(&delete, root.path())).await;

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let message = result.unwrap_err();
        assert!(message.contains("delete"), "got: {message}");
        assert!(delete.exists());
    }
}
