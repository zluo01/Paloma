mod process_controller;

use std::{io, path::PathBuf, str::FromStr};

use process_controller::{ProcessController, ProcessExecRequest};
use schemars::JsonSchema;
use scry_extension_base::{Capability, ExtensionService, ToolHandler};
use scry_extension_protocol::v1::{ToolContent, ToolFacet};
use scry_utils::init_logging;
use serde::Deserialize;
use uuid::Uuid;

pub const EXTENSION_ID: &str = "Shell";
pub const CAPABILITY_ID: &str = "Shell";

pub fn run() -> io::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            init_logging("info".into());
            service()?.serve().await
        })
}

fn service() -> io::Result<ExtensionService> {
    let capabilities: Vec<Box<dyn Capability>> = vec![Box::new(Shell::new())];

    Ok(ExtensionService::new(
        EXTENSION_ID,
        "Execute shell commands.",
        None,
        None,
        capabilities,
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShellArgs {
    /// argv array to execute. argv[0] is the program name (e.g. "git",
    /// "cargo", "bash"); the remaining elements are its positional
    /// arguments, one per element. Do NOT pre-concatenate multiple
    /// arguments into a single string — each whitespace-separated token
    /// is its own element.
    ///
    /// To use shell features (pipes, globs, redirection, env-var
    /// expansion, chained commands with `&&` or `;`), invoke a shell
    /// explicitly as argv[0]:
    ///   ["bash", "-lc", "pacman -Q | grep firefox"]
    ///   ["bash", "-lc", "git add . && git commit -m 'fix'"]
    ///
    /// Plain commands need no shell:
    ///   ["ls", "-la"]
    ///   ["cargo", "build", "--release"]
    ///   ["git", "status"]
    pub command: Vec<String>,
    /// Absolute path to the working directory in which to run the
    /// command. Must start with "/" — relative paths are rejected with
    /// an error and the call fails. Use this field to set the directory;
    /// do NOT invoke `cd` as part of the command. Embedding `cd` is
    /// redundant, error-prone (relative paths, missing quoting), and
    /// makes the executed argv harder for the user to audit.
    ///
    /// Examples: "/home/user/project", "/tmp", "/etc".
    pub workdir: String,
    /// Short, human-readable summary of what this command does. The UI
    /// displays this alongside the raw argv so the user can see at a
    /// glance what the assistant is doing.
    ///
    /// Recommended 5-10 words for simple commands; for complex commands
    /// with pipes or multiple operations, provide more context. Use
    /// present-tense, third-person voice describing the outcome, not the
    /// mechanics. Examples:
    ///   "Lists installed Firefox packages"
    ///   "Compiles the workspace in release mode"
    #[allow(dead_code)]
    pub description: String,
}

struct Shell {
    process_controller: ProcessController,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            process_controller: ProcessController::new(),
        }
    }
}

impl Capability for Shell {
    fn id(&self) -> &'static str {
        CAPABILITY_ID
    }

    fn description(&self) -> &str {
        "Execute a command and return stdout, stderr, and exit code."
    }

    fn tool_handler(&self) -> Option<&dyn ToolHandler> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl ToolHandler for Shell {
    fn facet(&self) -> ToolFacet {
        ToolFacet {
            description: include_str!("description.md").to_string(),
            parameters: serde_json::to_string(&schemars::schema_for!(ShellArgs))
                .expect("JsonSchema output is always serializable"),
        }
    }

    async fn invoke(
        &self,
        session_id: &str,
        call_id: &str,
        arguments: &str,
    ) -> Result<ToolContent, String> {
        let args: ShellArgs = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
        validate_argv(&args.command)?;
        let workdir = resolve_workdir(&args.workdir)?;
        let session_id = Uuid::from_str(session_id).map_err(|e| e.to_string())?;

        let request = ProcessExecRequest {
            session_id,
            call_id: call_id.to_string(),
            command: args.command,
            cwd: workdir,
        };

        self.process_controller
            .exec(request)
            .await
            .map_err(|e| e.to_string())
    }

    async fn cancel(&self, session_id: &str) -> Result<(), String> {
        let id = Uuid::from_str(session_id).map_err(|e| e.to_string())?;
        self.process_controller.cancel_session(id);
        Ok(())
    }
}

fn validate_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() {
        return Err("command argv is empty".to_string());
    }
    if argv[0].trim().is_empty() {
        return Err("command program (argv[0]) is empty".to_string());
    }
    Ok(())
}

fn resolve_workdir(workdir: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(workdir);
    if !path.is_absolute() {
        return Err(format!("workdir must be absolute, got: {workdir}"));
    }
    if !path.is_dir() {
        return Err(format!("workdir is not a directory: {workdir}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use scry_extension_protocol::v1::tool_content;

    use super::*;

    #[test]
    fn validate_argv_rejects_empty_argv() {
        assert_eq!(validate_argv(&[]).unwrap_err(), "command argv is empty");
    }

    #[test]
    fn validate_argv_rejects_empty_program() {
        assert_eq!(
            validate_argv(&["   ".to_string()]).unwrap_err(),
            "command program (argv[0]) is empty"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_relative_workdir() {
        let tool = Shell::new();
        let arguments = serde_json::json!({
            "command": ["printf", "ok"],
            "workdir": "relative",
            "description": "Prints ok for the relative-workdir test",
        })
        .to_string();
        let actual = tool
            .invoke(&Uuid::now_v7().to_string(), "call_1", &arguments)
            .await;

        assert_eq!(
            actual.unwrap_err(),
            "workdir must be absolute, got: relative"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_malformed_arguments() {
        let tool = Shell::new();
        let actual = tool
            .invoke(&Uuid::now_v7().to_string(), "call_1", "not json")
            .await;

        assert!(actual.is_err());
    }

    #[tokio::test]
    async fn invoke_delegates_to_process_manager() {
        let tool = Shell::new();
        let arguments = serde_json::json!({
            "command": ["printf", "ok"],
            "workdir": std::env::current_dir().unwrap().to_string_lossy(),
            "description": "Prints ok to verify process-manager delegation",
        })
        .to_string();
        let content = tool
            .invoke(&Uuid::now_v7().to_string(), "call_2", &arguments)
            .await
            .unwrap();

        assert_eq!(content.tag, "shell_output");
        assert!(
            content
                .attributes
                .iter()
                .any(|attribute| attribute.key == "exit_code" && attribute.value == "0"),
            "got: {content:?}"
        );
        let stdout = content
            .children()
            .iter()
            .find(|child| child.tag == "stdout")
            .expect("stdout child");
        assert_eq!(
            stdout.body,
            Some(tool_content::Body::Text("ok".to_string()))
        );
    }
}
