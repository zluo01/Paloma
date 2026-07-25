use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dashmap::DashMap;
use log::error;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    HealthLevel, HealthStatus, OAuthCallbackState, Plugin, PluginArgs, PluginType,
    capability::{ToolResult, ToolSchema, ToolSpec},
    db::{Storage, StorageError},
    mcp::{McpPlugin, McpPluginError, McpPluginInfo},
    utils::{OAuthError, finalize_oauth_connection, init_oauth_connection},
};

pub struct McpController {
    handlers: Arc<DashMap<String, McpHandler>>,
    sessions: DashMap<Uuid, CancellationToken>,
    storage: Storage,
    request_client: reqwest::Client,
}

impl McpController {
    pub async fn new(storage: Storage, request_client: reqwest::Client) -> Result<Self> {
        let mcp_plugins = storage.plugins_by_type(PluginType::Mcp).await?;

        // Connect configured MCP servers in the background so a slow or broken
        // server can neither fail nor delay startup.
        let handlers: Arc<DashMap<String, McpHandler>> = Arc::new(DashMap::new());
        for mcp_plugin in mcp_plugins {
            let handlers = Arc::clone(&handlers);
            let client = request_client.clone();
            let storage = storage.clone();
            tokio::spawn(async move {
                let (plugin, specs) = McpPlugin::new(&mcp_plugin, client, storage).await;
                handlers.insert(
                    mcp_plugin.name.clone(),
                    McpHandler {
                        description: plugin.description().to_string(),
                        connection: Arc::new(plugin),
                        specs,
                    },
                );
            });
        }

        Ok(Self {
            handlers,
            sessions: DashMap::new(),
            storage,
            request_client,
        })
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpPluginInfo>> {
        let plugins = self.storage.plugins_by_type(PluginType::Mcp).await?;
        Ok(plugins
            .into_iter()
            .map(|config| {
                // no handler yet: the background connect from `new` has not
                // settled, so the server is still starting
                let Some(handler) = self.handlers.get(&config.name) else {
                    return McpPluginInfo {
                        description: String::new(),
                        status: HealthStatus::Starting,
                        error: None,
                        config,
                    };
                };
                McpPluginInfo {
                    description: handler.description.clone(),
                    status: handler.connection.health_status(),
                    error: handler.connection.error().map(str::to_string),
                    config,
                }
            })
            .collect())
    }

    pub async fn init_connection(&self, config: Plugin) -> Result<Option<OAuthCallbackState>> {
        match config.args {
            PluginArgs::Remote {
                url,
                requires_auth: true,
            } => Ok(Some(init_oauth_connection(&url).await?)),
            _ => Ok(None),
        }
    }

    pub async fn finalize_connection(
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
        // the row must be in the db before register_mcp connects: an oauth
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
        if let Err(register_error) = self.register_mcp(&config).await {
            if let Err(e) = self.storage.delete_plugin(&config.name).await {
                error!(
                    "failed to remove plugin {} after failed connect: {e}",
                    config.name
                );
            }
            return Err(register_error);
        }
        Ok(())
    }

    async fn register_mcp(&self, config: &Plugin) -> Result<()> {
        let (plugin, specs) =
            McpPlugin::new(config, self.request_client.clone(), self.storage.clone()).await;
        // fail to init
        if plugin.health_status() != HealthStatus::Running {
            return Err(McpControllerError::FailToInitialize {
                reason: plugin.error().map(str::to_string),
            });
        }

        self.handlers.insert(
            config.name.clone(),
            McpHandler {
                description: plugin.description().to_string(),
                connection: Arc::new(plugin),
                specs,
            },
        );
        Ok(())
    }

    pub async fn remove_mcp(&self, name: &str) -> Result<()> {
        self.handlers.remove(name);
        self.storage.delete_plugin(name).await?;
        Ok(())
    }

    pub async fn update_mcp(&self, config: &Plugin) -> Result<()> {
        self.register_mcp(config).await?;
        self.storage
            .update_plugin(
                &config.name,
                config.transport,
                config.timeout,
                &config.env,
                &config.args,
            )
            .await?;
        Ok(())
    }

    pub async fn schemas(&self, disabled: &HashSet<String>) -> Vec<ToolSchema> {
        self.handlers
            .iter()
            .filter(|entry| !disabled.contains(entry.key()))
            .filter(|entry| entry.connection.health_status() == HealthStatus::Running)
            .flat_map(|entry| {
                entry
                    .specs
                    .values()
                    .map(|spec| spec.schema.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn health_level(&self) -> HealthLevel {
        HealthLevel::combine(
            self.handlers
                .iter()
                .map(|entry| entry.connection.health_status().into()),
        )
    }

    pub fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.handlers
            .iter()
            .find_map(|entry| entry.specs.get(name).cloned())
    }

    pub async fn call(
        &self,
        name: String, // encoded mcp function call name
        session_id: Uuid,
        call_id: String,
        args: Value,
    ) -> Result<ToolResult> {
        let target = self.handlers.iter().find_map(|entry| {
            entry
                .specs
                .get(&name)
                .map(|spec| (Arc::clone(&entry.connection), spec.tool.clone()))
        });

        let Some((connection, tool)) = target else {
            return Err(McpControllerError::UnknownTool(name));
        };

        let token = self.sessions.entry(session_id).or_default().clone();
        Ok(connection.call(tool, token, call_id, args).await?)
    }

    pub fn cancel_session(&self, session_id: Uuid) {
        if let Some((_, token)) = self.sessions.remove(&session_id) {
            token.cancel();
        }
    }
}

struct McpHandler {
    description: String,
    specs: HashMap<String, ToolSpec>,
    connection: Arc<McpPlugin>,
}

#[derive(Debug, thiserror::Error)]
pub enum McpControllerError {
    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    OAuth(#[from] OAuthError),

    #[error(transparent)]
    Plugin(#[from] McpPluginError),

    #[error("unknown mcp tool: {0}")]
    UnknownTool(String),

    #[error("fail to initialize mcp plugin: {}", reason.as_deref().unwrap_or("unknown error"))]
    FailToInitialize { reason: Option<String> },
}

type Result<T> = std::result::Result<T, McpControllerError>;
