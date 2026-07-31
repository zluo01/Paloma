use uuid::Uuid;

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

#[derive(Clone, PartialEq)]
pub enum CommandType {
    Simple,
    Composite,
}

#[derive(Clone)]
pub struct PermissionDecision {
    t: CommandType,
    parsed_commands: Vec<Vec<String>>,
    decision: ArgvDecision,
}

impl PermissionDecision {
    pub fn new(t: CommandType, parsed_commands: Vec<Vec<String>>, decision: ArgvDecision) -> Self {
        Self {
            t,
            parsed_commands,
            decision,
        }
    }

    pub fn command_type(&self) -> &CommandType {
        &self.t
    }

    pub fn parsed_commands(&self) -> &[Vec<String>] {
        &self.parsed_commands
    }

    pub fn decision(&self) -> ArgvDecision {
        self.decision
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum PermissionState {
    Allow,
    Deny,
    Error,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum UserDecision {
    AllowOnce {
        call_id: String,
    },
    Allow {
        call_id: String,
        command: String,
        glob: bool,
    },
    AllowSession {
        session_id: Uuid,
        call_id: String,
    },
    IgnorePermission {
        session_id: Uuid,
        call_id: String,
    },
    Deny {
        call_id: String,
    },
}
