mod entity;
mod error;
mod parser;
mod safety;
mod utils;

pub use entity::ArgvDecision;
pub use error::{PermissionError, Result};
use scry_storage::db::Storage;

pub struct PermissionController {
    storage: Storage,
}

impl PermissionController {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn classify(&self, command: &[String]) -> Result<ArgvDecision> {
        let Some(commands) = parser::parse_commands(command)? else {
            // Unparseable composite: always ask, never persist globally.
            return Ok(ArgvDecision::AskNoPersist);
        };
        // More than one atom means a composite; only a single atom is eligible for a global "allow always".
        let is_composite = commands.len() > 1;
        let mut strictest: Option<ArgvDecision> = None;
        for argv in commands {
            let decision = self.decide(argv).await?;
            if strictest.is_none_or(|prev| decision.severity() > prev.severity()) {
                strictest = Some(decision);
            }
        }
        let folded = strictest.ok_or(PermissionError::EmptyCommand)?;
        // A composite with a novel atom must not offer global persistence;
        // downgrade `Unknown` to ask-no-persist. All atoms allowlisted stays
        // `Allow` (auto-run); dangerous/not-executable atoms already dominate.
        if is_composite && folded == ArgvDecision::Unknown {
            Ok(ArgvDecision::AskNoPersist)
        } else {
            Ok(folded)
        }
    }

    pub async fn add_permission(&self, prefix: &[String], with_glob: bool) -> Result<()> {
        self.storage
            .add_permission(&prefix.join(" "), with_glob)
            .await?;
        Ok(())
    }

    async fn decide(&self, argv: Vec<String>) -> Result<ArgvDecision> {
        let decision = safety::safety_check(&argv)?;
        if decision == ArgvDecision::Unknown
            && self.storage.is_command_allowed(&argv.join(" ")).await?
        {
            return Ok(ArgvDecision::Allow);
        }
        Ok(decision)
    }
}
