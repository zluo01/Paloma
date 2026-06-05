#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgvDecision {
    /// Built-in safe-read allowlist hit. No prompt.
    Allow = 0,
    /// Neither dangerous nor on the safe allowlist. Ask the user
    /// (orchestrator first consults consensus DB).
    Unknown = 1,
    /// Dangerous shape; prompt with once/deny only, never persist.
    AskNoPersist = 2,
    /// Cannot be executed via the non-interactive exec path; refuse outright.
    NotExecutable = 3,
}

impl ArgvDecision {
    /// Aggregation key: higher = stricter verdict.
    pub fn severity(self) -> u8 {
        self as u8
    }
}

#[derive(Clone)]
pub enum CommandType {
    Simple,
    Composite,
}

#[derive(Clone)]
pub struct PermissionDecision {
    t: CommandType,
    decision: ArgvDecision,
}

impl PermissionDecision {
    pub fn new(t: CommandType, decision: ArgvDecision) -> Self {
        Self { t, decision }
    }

    pub fn command_type(&self) -> &CommandType {
        &self.t
    }

    pub fn decision(&self) -> ArgvDecision {
        self.decision
    }
}
