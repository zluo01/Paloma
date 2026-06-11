mod constants;
mod entity;
mod error;
mod parser;
mod safety;
mod transparent;

pub use entity::ArgvDecision;
pub use error::{PermissionError, Result};
use scry_storage::Storage;

pub use crate::entity::{CommandType, PermissionDecision};
use crate::parser::try_parse_shell;
use crate::transparent::{strip_transparent_command, StripTransparentWrapperError};

pub struct PermissionController {
    storage: Storage,
}

impl PermissionController {
    pub fn new(storage: Storage) -> Self {
        // call on start up to panic on any tree-sitter parser config issues.
        let _ = try_parse_shell("true");

        Self { storage }
    }

    pub async fn classify(&self, command: &[String]) -> Result<PermissionDecision> {
        let Some(commands) = parser::parse_commands(command)? else {
            // Unparseable composite: always ask, never persist globally.
            return Ok(PermissionDecision::new(
                CommandType::Composite,
                ArgvDecision::AskNoPersist,
            ));
        };

        // we only strip the transparent if the command is a simple command.
        if commands.len() == 1 {
            return match strip_transparent_command(&commands[0]) {
                Ok(inner) => {
                    let decision = self.decide(inner).await?;
                    Ok(PermissionDecision::new(CommandType::Simple, decision))
                },
                // Malformed wrapper invocation — the real tool would reject it.
                Err(
                    e @ (StripTransparentWrapperError::MissingFlagValue { .. }
                    | StripTransparentWrapperError::InvalidFlagValue { .. }
                    | StripTransparentWrapperError::InvalidCommand { .. }),
                ) => Err(PermissionError::InvalidCommand(e.to_string())),
                // Not sure about the result, always ask.
                Err(
                    StripTransparentWrapperError::UnknownOption { .. }
                    | StripTransparentWrapperError::UnsafeToStrip { .. }
                    | StripTransparentWrapperError::DepthExceeded,
                ) => Ok(PermissionDecision::new(
                    CommandType::Simple,
                    ArgvDecision::AskNoPersist,
                )),
            };
        }

        // Composite
        let mut strictest: Option<ArgvDecision> = None;
        for argv in commands {
            let decision = self.decide(&argv).await?;
            if strictest.is_none_or(|prev| decision.severity() > prev.severity()) {
                strictest = Some(decision);
            }
        }
        let folded = strictest.ok_or(PermissionError::EmptyCommand)?;
        // A composite with a novel atom must not offer global persistence;
        // downgrade `Unknown` to ask-no-persist. All atoms allowlisted stays
        // `Allow` (auto-run); dangerous/not-executable atoms already dominate.
        let decision = if folded == ArgvDecision::Unknown {
            ArgvDecision::AskNoPersist
        } else {
            folded
        };
        Ok(PermissionDecision::new(CommandType::Composite, decision))
    }

    pub async fn add_permission(&self, prefix: String, with_glob: bool) -> Result<()> {
        self.storage.add_permission(&prefix, with_glob).await?;
        Ok(())
    }

    async fn decide(&self, argv: &[String]) -> Result<ArgvDecision> {
        let decision = safety::safety_check(argv)?;
        if decision == ArgvDecision::Unknown
            && self.storage.is_command_allowed(&argv.join(" ")).await?
        {
            return Ok(ArgvDecision::Allow);
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    async fn fresh_storage() -> Storage {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(":memory:"))
            .await
            .expect("in-memory pool");
        Storage::from_pool(pool).await.expect("Storage::from_pool")
    }

    async fn classify(parts: &[&str]) -> Result<PermissionDecision> {
        let controller = PermissionController::new(fresh_storage().await);
        controller.classify(&argv(parts)).await
    }

    #[tokio::test]
    async fn classifies_bash_lc_single_atom_as_simple_unknown() {
        let controller = PermissionController::new(fresh_storage().await);
        let decision = controller
            .classify(&argv(&[
                "bash",
                "-lc",
                "timeout 75 aria2c --dir=/tmp 'magnet:?xt=urn:btih:123456'",
            ]))
            .await
            .expect("classify");

        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::Unknown);
    }

    #[tokio::test]
    async fn classifies_sudo_with_wrapper_should_be_not_executable() {
        let controller = PermissionController::new(fresh_storage().await);
        let decision = controller
            .classify(&argv(&["bash", "-lc", "timeout 75 sudo a b"]))
            .await
            .expect("classify");

        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::NotExecutable);
    }

    #[tokio::test]
    async fn unparseable_complex_command_is_ask_no_persist() {
        // A complex script (control flow, substitutions, …) can't be split
        // into atoms, so it always asks and never persists.
        let decision = classify(&[
            "bash",
            "-lc",
            "for d in /tmp/*; do echo \"$d\"; done",
        ])
        .await
        .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Composite));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }

    // ------------------------------------------------------------------
    // composite folding
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn composite_all_safe_is_allow() {
        let decision = classify(&["bash", "-lc", "ls && pwd"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Composite));
        assert_eq!(decision.decision(), ArgvDecision::Allow);
    }

    #[tokio::test]
    async fn composite_with_unknown_is_ask_no_persist() {
        let decision = classify(&["bash", "-lc", "ls && cargo build"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Composite));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }

    #[tokio::test]
    async fn composite_with_dangerous_is_ask_no_persist() {
        let decision = classify(&["bash", "-lc", "ls && rm -rf x"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Composite));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }

    #[tokio::test]
    async fn composite_with_not_executable_is_not_executable() {
        let decision = classify(&["bash", "-lc", "ls && sudo rm x"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Composite));
        assert_eq!(decision.decision(), ArgvDecision::NotExecutable);
    }

    // ------------------------------------------------------------------
    // single command: stripping outcomes
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn simple_wrapped_safe_command_is_allow() {
        let decision = classify(&["timeout", "5", "ls"]).await.expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::Allow);
    }

    #[tokio::test]
    async fn simple_missing_flag_value_is_invalid_command() {
        let result = classify(&["timeout", "-s"]).await;
        assert!(matches!(result, Err(PermissionError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn simple_invalid_flag_value_is_invalid_command() {
        let result = classify(&["timeout", "-s", "cargo", "test"]).await;
        assert!(matches!(result, Err(PermissionError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn simple_invalid_inner_command_is_invalid_command() {
        let result = classify(&["timeout", "5x", "ls"]).await;
        assert!(matches!(result, Err(PermissionError::InvalidCommand(_))));
    }

    #[tokio::test]
    async fn simple_unknown_option_is_ask_no_persist() {
        let decision = classify(&["timeout", "--frobnicate", "5", "ls"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }

    #[tokio::test]
    async fn simple_unsafe_to_strip_is_ask_no_persist() {
        let decision = classify(&["env", "-C", "/tmp", "make"])
            .await
            .expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }

    #[tokio::test]
    async fn simple_depth_exceeded_is_ask_no_persist() {
        let mut parts = vec!["nohup"; 9];
        parts.push("ls");
        let decision = classify(&parts).await.expect("classify");
        assert!(matches!(decision.command_type(), CommandType::Simple));
        assert_eq!(decision.decision(), ArgvDecision::AskNoPersist);
    }
}
