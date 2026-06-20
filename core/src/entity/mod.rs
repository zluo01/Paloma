use std::{collections::HashMap, fmt};

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
    ClaudeCode,
    OpenAI,
    Anthropic,
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ProviderId::Codex => "Codex",
            ProviderId::ClaudeCode => "Claude Code",
            ProviderId::OpenAI => "OpenAI",
            ProviderId::Anthropic => "Anthropic",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum PluginType {
    Native,
    Mcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum Transport {
    Local,
    Http,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginArgs {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct Plugin {
    pub name: String,
    pub transport: Transport,
    pub timeout: i64,
    pub disabled: bool,
    #[sqlx(json)]
    pub env: HashMap<String, String>,
    #[sqlx(json)]
    pub args: PluginArgs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HealthStatus {
    Running = 0,
    Unhealthy = 1,
}

impl HealthStatus {
    /// Reconstruct from the `u8` held in an `AtomicU8`. Unknown values map to
    /// `Unhealthy` (fail-safe: don't use a tool in an unclear state).
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => HealthStatus::Running,
            _ => HealthStatus::Unhealthy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthLevel {
    Inactive,
    Healthy,
    Degraded,
    Down,
}

impl HealthLevel {
    pub fn from_counts(total: usize, healthy: usize) -> Self {
        match (total, healthy) {
            (0, _) => HealthLevel::Inactive,
            (total, healthy) if healthy == total => HealthLevel::Healthy,
            (_, 0) => HealthLevel::Down,
            _ => HealthLevel::Degraded,
        }
    }
}
