use std::sync::Arc;

use log::error;
use scry_capability::HealthStatus;
use scry_storage::{Plugin, PluginType, Storage, StorageError};

use crate::{remote::ToolControllerError, HealthLevel, ToolController};

pub struct McpServer {
    pub config: Plugin,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
}

pub struct PluginConnectionController {
    storage: Storage,
    tool_controller: Arc<ToolController>,
}

impl PluginConnectionController {
    pub fn new(storage: Storage, tool_controller: Arc<ToolController>) -> Self {
        Self {
            storage,
            tool_controller,
        }
    }

    /// aggregate health of all plugins
    pub async fn health_level(&self) -> HealthLevel {
        let servers = match self.list_mcps().await {
            Ok(servers) => servers,
            Err(e) => {
                error!("plugin health_level: {e}");
                return HealthLevel::Inactive;
            },
        };
        let healthy = servers
            .iter()
            .filter(|server| server.status == HealthStatus::Running)
            .count();
        HealthLevel::from_counts(servers.len(), healthy)
    }

    /// list all configured MCP servers
    pub async fn list_mcps(&self) -> Result<Vec<McpServer>> {
        let status = self.tool_controller.get_tools_status().await;
        let plugins = self.storage.all_mcp_plugins().await?;
        Ok(plugins
            .into_iter()
            .filter_map(|config| {
                let Some(tool) = status.get(&config.name) else {
                    error!(
                        "no live tool status for plugin {}; This indicates a bug. Skipping",
                        config.name
                    );
                    return None;
                };
                Some(McpServer {
                    description: tool.description.clone(),
                    status: tool.status,
                    error: tool.error.clone(),
                    config,
                })
            })
            .collect())
    }

    pub async fn add_mcp(&self, config: Plugin) -> Result<()> {
        self.tool_controller.register_tool(&config).await?;
        Ok(())
    }

    pub async fn remove_plugin(&self, name: &str, plugin_type: PluginType) -> Result<()> {
        match plugin_type {
            PluginType::Native => error!("Not yet implemented."),
            PluginType::Mcp => self.tool_controller.deregister_tool(name).await?,
        }
        Ok(())
    }

    pub async fn update_plugin(&self, plugin_type: PluginType, plugin: Plugin) -> Result<()> {
        match plugin_type {
            PluginType::Native => error!("Not yet implemented."),
            PluginType::Mcp => self.tool_controller.update_tool(&plugin).await?,
        }
        Ok(())
    }

    pub async fn toggle_plugin(&self, name: &str, disabled: bool) -> Result<()> {
        self.storage.toggle_plugin(name, disabled).await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginConnectionError {
    #[error(transparent)]
    ToolController(#[from] ToolControllerError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub type Result<T> = std::result::Result<T, PluginConnectionError>;
