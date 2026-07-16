use std::{collections::HashMap, fmt};

use scry_provider_protocol::v1::ProviderHealthStatus;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, PartialEq, Eq, Hash, Debug, FromRow, Serialize, Deserialize)]
pub struct ProviderBackendId {
    pub provider_id: String,
    pub backend_id: String,
}

impl fmt::Display for ProviderBackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} - {}", self.provider_id, self.backend_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum PluginType {
    Native,
    Provider,
    Mcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum Transport {
    Local,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginArgs {
    Local { command: String, args: Vec<String> },
    Remote { url: String, requires_auth: bool },
}

#[derive(Clone, FromRow)]
pub struct Plugin {
    pub name: String,
    pub transport: Transport,
    pub timeout: u32,
    pub disabled: bool,
    #[sqlx(json)]
    pub env: HashMap<String, String>,
    #[sqlx(json)]
    pub args: PluginArgs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HealthStatus {
    Starting = 0,
    Running = 1,
    Unhealthy = 2,
}

impl HealthStatus {
    /// Reconstruct from the `u8` held in an `AtomicU8`. Unknown values map to
    /// `Unhealthy` (fail-safe: don't use a tool in an unclear state).
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => HealthStatus::Starting,
            1 => HealthStatus::Running,
            _ => HealthStatus::Unhealthy,
        }
    }
}

impl From<ProviderHealthStatus> for HealthStatus {
    fn from(status: ProviderHealthStatus) -> Self {
        match status {
            ProviderHealthStatus::Starting => HealthStatus::Starting,
            ProviderHealthStatus::Running => HealthStatus::Running,
            // fail-safe: an unknown state is treated as unhealthy
            ProviderHealthStatus::Unknown | ProviderHealthStatus::Unhealthy => {
                HealthStatus::Unhealthy
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
