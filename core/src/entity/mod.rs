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

#[derive(Clone, Debug, Default)]
pub struct ExtensionCapabilityId {
    pub extension_id: String,
    pub capability_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum PluginType {
    Extension,
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

impl Plugin {
    pub(crate) fn builtin(name: &str, flag: &str) -> Self {
        let command = std::env::current_exe()
            .unwrap_or_else(|_| {
                panic!("current executable path should be resolvable for plugin {name}.")
            })
            .to_string_lossy()
            .into_owned();
        Self {
            name: name.to_string(),
            transport: Transport::Local,
            // no global timeout in built-in plugins
            timeout: 0,
            disabled: false,
            env: HashMap::new(),
            args: PluginArgs::Local {
                command,
                args: vec![flag.to_string(), name.to_string()],
            },
        }
    }
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
#[repr(u8)]
pub enum HealthLevel {
    Inactive = 0b00,
    Healthy = 0b01,
    Down = 0b10,
    Degraded = 0b11,
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

    pub fn combine(levels: impl IntoIterator<Item = HealthLevel>) -> Self {
        match levels.into_iter().fold(0u8, |acc, level| acc | level as u8) {
            0b00 => HealthLevel::Inactive,
            0b01 => HealthLevel::Healthy,
            0b10 => HealthLevel::Down,
            _ => HealthLevel::Degraded,
        }
    }
}

impl From<HealthStatus> for HealthLevel {
    fn from(status: HealthStatus) -> Self {
        match status {
            HealthStatus::Starting => HealthLevel::Inactive,
            HealthStatus::Running => HealthLevel::Healthy,
            HealthStatus::Unhealthy => HealthLevel::Down,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    Name(String),
    Path(String),
    Embedded(Vec<u8>),
}
