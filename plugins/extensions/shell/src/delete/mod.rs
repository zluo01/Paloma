use std::path::Path;

use paloma_extension_base::{Capability, ToolHandler};
use paloma_extension_protocol::v1::{ToolContent, ToolFacet};
use schemars::JsonSchema;
use serde::Deserialize;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

pub const CAPABILITY_ID: &str = "DeleteFiles";

#[cfg(unix)]
const DESCRIPTION: &str = include_str!("description.md");
#[cfg(windows)]
const DESCRIPTION: &str = include_str!("description_windows.md");

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteFilesArgs {
    /// Absolute paths of the files or directories to move to the trash.
    /// A relative path fails the whole call before anything is touched.
    /// Symbolic links are trashed as links; their target is never
    /// followed.
    pub paths: Vec<String>,
}

pub(crate) struct DeleteFiles;

impl DeleteFiles {
    pub fn new() -> Self {
        Self
    }
}

impl Capability for DeleteFiles {
    fn id(&self) -> &'static str {
        CAPABILITY_ID
    }

    fn description(&self) -> &str {
        "Move files or directories to the trash."
    }

    fn tool_handler(&self) -> Option<&dyn ToolHandler> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl ToolHandler for DeleteFiles {
    fn facet(&self) -> ToolFacet {
        ToolFacet {
            description: DESCRIPTION.to_string(),
            short_description: "Move files or directories to the trash.".to_string(),
            parameters: serde_json::to_string(&schemars::schema_for!(DeleteFilesArgs))
                .expect("JsonSchema output is always serializable"),
        }
    }

    async fn invoke(
        &self,
        _session_id: &str,
        _call_id: &str,
        arguments: &str,
    ) -> Result<ToolContent, String> {
        let args: DeleteFilesArgs = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
        validate_paths(&args.paths).map_err(|e| e.to_string())?;

        let mut content = ToolContent::new("delete_output");
        match platform::trash(&args.paths).await {
            Ok(failures) => {
                for (target, message) in failures {
                    content = content.child(
                        ToolContent::new("failed")
                            .attr("target", target)
                            .cdata(message),
                    );
                }
            },
            Err(e) => content = content.child(ToolContent::new("error").cdata(e.to_string())),
        }
        Ok(content)
    }

    async fn cancel(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

fn validate_paths(paths: &[String]) -> Result<(), DeleteError> {
    if paths.is_empty() {
        return Err(DeleteError::EmptyPaths);
    }
    for path in paths {
        if !Path::new(path).is_absolute() {
            return Err(DeleteError::RelativePath(path.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeleteError {
    #[error("paths is empty")]
    EmptyPaths,
    #[error("path must be absolute, got: {0}")]
    RelativePath(String),
    #[cfg(target_os = "linux")]
    #[error("gio was not found on the host")]
    GioNotFound,
    #[cfg(not(target_os = "linux"))]
    #[error("the trash task failed: {0}")]
    TaskFailed(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_paths_rejects_empty_list() {
        assert_eq!(
            validate_paths(&[]).unwrap_err().to_string(),
            "paths is empty"
        );
    }

    #[test]
    fn validate_paths_rejects_relative_path() {
        let absolute = std::env::temp_dir()
            .join("ok")
            .to_string_lossy()
            .into_owned();
        let paths = [absolute, "relative".to_string()];
        assert_eq!(
            validate_paths(&paths).unwrap_err().to_string(),
            "path must be absolute, got: relative"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_relative_paths_before_deleting() {
        let tool = DeleteFiles::new();
        let arguments = serde_json::json!({ "paths": ["relative"] }).to_string();
        let actual = tool.invoke("session", "call_1", &arguments).await;

        assert_eq!(actual.unwrap_err(), "path must be absolute, got: relative");
    }

    #[tokio::test]
    async fn invoke_reports_missing_path_per_entry() {
        let tool = DeleteFiles::new();
        let missing = std::env::temp_dir().join(format!("paloma-missing-{}", uuid::Uuid::now_v7()));
        let arguments = serde_json::json!({ "paths": [missing.to_string_lossy()] }).to_string();
        let content = tool.invoke("session", "call_1", &arguments).await.unwrap();

        assert_eq!(content.tag, "delete_output");
        let entry = &content.children()[0];
        assert_eq!(entry.tag, "failed");
        assert!(
            entry
                .attributes
                .iter()
                .any(|a| a.key == "target" && a.value == missing.to_string_lossy()),
            "got: {entry:?}"
        );
    }
}
