mod process_manager;

use std::path::PathBuf;

pub use process_manager::{ProcessExecRequest, ProcessManager, ProcessManagerClient};
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

use crate::entity::{Capability, CapabilityMeta, Tool, ToolResult};

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
    ///   "Searches the codebase for TODO comments"
    pub description: String,
}

pub struct Shell {
    client: ProcessManagerClient,
}

impl Shell {
    pub fn new(client: ProcessManagerClient) -> Self {
        Self { client }
    }
}

impl Capability for Shell {
    fn id(&self) -> &'static str {
        "shell"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "Shell".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Execute a command and return stdout, stderr, and exit code.".to_string(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

#[async_trait::async_trait]
impl Tool for Shell {
    type Args = ShellArgs;

    const NAME: &'static str = "shell";
    const DESCRIPTION: &'static str = include_str!("description.md");

    async fn invoke(
        &self,
        session_id: Uuid,
        call_id: String,
        args: Self::Args,
    ) -> Result<ToolResult, String> {
        validate_argv(&args.command)?;
        let workdir = resolve_workdir(&args.workdir)?;

        let request = ProcessExecRequest {
            session_id,
            call_id,
            command: args.command,
            cwd: workdir,
        };

        self.client.exec(request).await.map_err(|e| e.to_string())
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
    use super::{process_manager::ProcessManager, *};

    fn spawn_shell() -> Shell {
        let (mut pm, client) = ProcessManager::new();
        tokio::spawn(async move { pm.run().await });
        Shell::new(client)
    }

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
        let tool = spawn_shell();
        let actual = tool
            .invoke(
                Uuid::now_v7(),
                "call_1".to_string(),
                ShellArgs {
                    command: vec!["printf".to_string(), "ok".to_string()],
                    workdir: "relative".to_string(),
                    description: "Prints ok for the relative-workdir test".to_string(),
                },
            )
            .await;

        assert_eq!(
            actual.unwrap_err(),
            "workdir must be absolute, got: relative"
        );
    }

    #[tokio::test]
    async fn invoke_delegates_to_process_manager() {
        let tool = spawn_shell();
        let result = tool
            .invoke(
                Uuid::now_v7(),
                "call_2".to_string(),
                ShellArgs {
                    command: vec!["printf".to_string(), "ok".to_string()],
                    workdir: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    description: "Prints ok to verify process-manager delegation".to_string(),
                },
            )
            .await
            .unwrap();

        let ToolResult::Text(text) = result else {
            panic!("expected text result");
        };
        assert!(text.contains("exit_code=\"0\""), "got:\n{text}");
        assert!(text.contains("<![CDATA[ok]]>"), "got:\n{text}");
    }
}
