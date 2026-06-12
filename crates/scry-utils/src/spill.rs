use std::path::{Path, PathBuf};

/// Write a complete oversized tool payload to `<root>/<call_id>/<name>`,
/// creating the directory as needed. Returns the file path, or `None`
/// (after logging) when the directory or file cannot be written — callers
/// degrade to inline-prefix-only output.
pub async fn write_spill_file(
    root: &Path,
    call_id: &str,
    name: &str,
    content: &[u8],
) -> Option<PathBuf> {
    let dir = root.join(call_id);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        log::error!(
            "could not create spill dir {}: {e}; overflow will be discarded",
            dir.display()
        );
        return None;
    }

    let path = dir.join(name);
    match tokio::fs::write(&path, content).await {
        Ok(()) => Some(path),
        Err(e) => {
            log::error!(
                "could not write spill file {}: {e}; overflow will be discarded",
                path.display()
            );
            None
        },
    }
}
