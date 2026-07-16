use std::sync::Arc;

use log::error;

use crate::{
    controller::{
        ProviderController, ProviderControllerError, ToolController, ToolControllerError,
    },
    db::{Storage, StorageError},
    entity::{HealthLevel, HealthStatus, Plugin, PluginArgs, PluginType},
    utils::{OAuthCallbackState, OAuthError, finalize_oauth_connection, init_oauth_connection},
};

#[derive(Clone)]
pub struct McpServer {
    pub config: Plugin,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
}

pub struct PluginConnectionController {
    storage: Storage,
    tool_controller: Arc<ToolController>,
    provider_controller: Arc<ProviderController>,
}

impl PluginConnectionController {
    pub fn new(
        storage: Storage,
        tool_controller: Arc<ToolController>,
        provider_controller: Arc<ProviderController>,
    ) -> Self {
        Self {
            storage,
            tool_controller,
            provider_controller,
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
        // servers still connecting don't count against health
        let (settled, healthy) =
            servers
                .iter()
                .fold((0, 0), |(settled, healthy), server| match server.status {
                    HealthStatus::Starting => (settled, healthy),
                    HealthStatus::Running => (settled + 1, healthy + 1),
                    HealthStatus::Unhealthy => (settled + 1, healthy),
                });
        HealthLevel::from_counts(settled, healthy)
    }

    /// list all configured MCP servers
    pub async fn list_mcps(&self) -> Result<Vec<McpServer>> {
        let status = self.tool_controller.get_tools_status().await;
        let plugins = self.storage.plugins_by_type(PluginType::Mcp).await?;
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

    pub async fn init_mcp_connection(&self, config: Plugin) -> Result<Option<OAuthCallbackState>> {
        match config.args {
            PluginArgs::Remote {
                url,
                requires_auth: true,
            } => Ok(Some(init_oauth_connection(&url).await?)),
            _ => Ok(None),
        }
    }

    pub async fn finalize_mcp_connection(
        &self,
        config: Plugin,
        state: Option<OAuthCallbackState>,
    ) -> Result<()> {
        let credential = match state {
            Some(state) => Some(
                serde_json::to_value(finalize_oauth_connection(state).await?)
                    .map_err(StorageError::from)?,
            ),
            None => None,
        };
        // the row must be in the db before register_tool connects: an oauth
        // connect loads its credential from there
        self.storage
            .insert_plugin(
                &config.name,
                PluginType::Mcp,
                config.transport,
                config.timeout,
                &config.env,
                &config.args,
                credential.as_ref(),
            )
            .await?;
        // roll the row back if the connect fails so a bad config isn't left behind
        if let Err(register_error) = self.tool_controller.register_tool(&config).await {
            if let Err(e) = self.storage.delete_plugin(&config.name).await {
                error!(
                    "failed to remove plugin {} after failed connect: {e}",
                    config.name
                );
            }
            return Err(register_error.into());
        }
        Ok(())
    }

    pub async fn remove_plugin(&self, plugin_type: PluginType, name: &str) -> Result<()> {
        match plugin_type {
            PluginType::Native => error!("Not yet implemented."),
            PluginType::Mcp => self.tool_controller.deregister_tool(name).await?,
            PluginType::Provider => self.provider_controller.remove_provider(name).await?,
        }
        Ok(())
    }

    pub async fn update_plugin(&self, plugin_type: PluginType, plugin: Plugin) -> Result<()> {
        match plugin_type {
            PluginType::Native => error!("Not yet implemented."),
            PluginType::Mcp => self.tool_controller.update_tool(&plugin).await?,
            PluginType::Provider => self.provider_controller.update_provider(&plugin).await?,
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
    ProviderController(#[from] ProviderControllerError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    OAuth(#[from] OAuthError),
}

type Result<T> = std::result::Result<T, PluginConnectionError>;
