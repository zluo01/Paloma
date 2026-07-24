use std::sync::Arc;

use log::error;

use crate::{
    controller::{
        ExtensionControllerError, ProviderController, ProviderControllerError,
        remote::{ExtensionController, McpController, McpControllerError},
    },
    db::{Storage, StorageError},
    entity::{HealthLevel, HealthStatus, Plugin, PluginType},
};

pub struct PluginConnectionController {
    storage: Storage,
    mcp_controller: Arc<McpController>,
    provider_controller: Arc<ProviderController>,
    extension_controller: Arc<ExtensionController>,
}

impl PluginConnectionController {
    pub fn new(
        storage: Storage,
        mcp_controller: Arc<McpController>,
        provider_controller: Arc<ProviderController>,
        extension_controller: Arc<ExtensionController>,
    ) -> Self {
        Self {
            storage,
            mcp_controller,
            provider_controller,
            extension_controller,
        }
    }

    /// aggregate health of all plugins
    pub async fn health_level(&self) -> HealthLevel {
        let servers = match self.mcp_controller.list_mcps().await {
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

    pub async fn remove_plugin(&self, plugin_type: PluginType, name: &str) -> Result<()> {
        match plugin_type {
            PluginType::Extension => self.extension_controller.remove_extension(name).await?,
            PluginType::Mcp => self.mcp_controller.remove_mcp(name).await?,
            PluginType::Provider => self.provider_controller.remove_provider(name).await?,
        }
        Ok(())
    }

    pub async fn update_plugin(&self, plugin_type: PluginType, plugin: Plugin) -> Result<()> {
        match plugin_type {
            PluginType::Extension => self.extension_controller.update_extension(&plugin).await?,
            PluginType::Mcp => self.mcp_controller.update_mcp(&plugin).await?,
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
    McpController(#[from] McpControllerError),

    #[error(transparent)]
    ProviderController(#[from] ProviderControllerError),

    #[error(transparent)]
    ExtensionController(#[from] ExtensionControllerError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

type Result<T> = std::result::Result<T, PluginConnectionError>;
