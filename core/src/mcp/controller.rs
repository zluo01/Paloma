use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use log::error;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{McpPlugin, McpPluginError, McpPluginInfo, McpToolSpecCache};
use crate::{
    CapabilityFacet, HealthLevel, HealthStatus, OAuthCallbackState, Plugin, PluginArgs, PluginType,
    db::{Storage, StorageError},
    entity::{CapabilityInfo, ToolResult, ToolSchema, ToolSpec},
    utils::{OAuthError, finalize_oauth_connection, init_oauth_connection},
};

pub struct McpController {
    handlers: Arc<DashMap<String, McpHandler>>,
    sessions: DashMap<Uuid, CancellationToken>,
    specs_cache: Arc<McpToolSpecCache>,
    storage: Storage,
    request_client: reqwest::Client,
}

impl McpController {
    pub async fn new(storage: Storage, request_client: reqwest::Client) -> Result<Self> {
        let mcp_plugins = storage.plugins_by_type(PluginType::Mcp).await?;

        // Connect configured MCP servers in the background so a slow or broken
        // server can neither fail nor delay startup.
        let handlers: Arc<DashMap<String, McpHandler>> = Arc::new(DashMap::new());
        let specs_cache = McpToolSpecCache::new();
        for mcp_plugin in mcp_plugins {
            let handlers = Arc::clone(&handlers);
            let specs_cache = Arc::clone(&specs_cache);
            let client = request_client.clone();
            let storage = storage.clone();
            tokio::spawn(async move {
                let (plugin, specs) = McpPlugin::new(&mcp_plugin, client, storage).await;
                specs_cache.insert(mcp_plugin.name.clone(), specs).await;
                handlers.insert(
                    mcp_plugin.name.clone(),
                    McpHandler {
                        description: plugin.description().to_string(),
                        connection: Arc::new(plugin),
                    },
                );
            });
        }

        Ok(Self {
            handlers,
            sessions: DashMap::new(),
            specs_cache,
            storage,
            request_client,
        })
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpPluginInfo>> {
        let plugins = self.storage.plugins_by_type(PluginType::Mcp).await?;
        let disabled = self
            .storage
            .disabled_capabilities(&[CapabilityFacet::Mcp])
            .await?;
        let mut infos = Vec::with_capacity(plugins.len());
        for config in plugins {
            // following will temporarily hold the handlers map shard lock until the end of the scope
            // should be fine as handlers is read more write less
            let Some(handler) = self.handlers.get(&config.name) else {
                infos.push(McpPluginInfo {
                    description: String::new(),
                    status: HealthStatus::Starting,
                    error: None,
                    tools: vec![],
                    config,
                });
                continue;
            };

            infos.push(McpPluginInfo {
                description: handler.description.clone(),
                status: handler.connection.health_status(),
                error: handler.connection.error().map(str::to_string),
                tools: tool_infos(
                    &config.name,
                    self.specs_cache.peek(&config.name).await,
                    &disabled,
                ),
                config,
            });
        }
        Ok(infos)
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

        self.specs_cache.insert(config.name.clone(), specs).await;
        self.handlers.insert(
            config.name.clone(),
            McpHandler {
                description: plugin.description().to_string(),
                connection: Arc::new(plugin),
            },
        );
        Ok(())
    }

    pub async fn remove_mcp(&self, name: &str) -> Result<()> {
        if let Some((_, handler)) = self.handlers.remove(name) {
            handler.connection.shutdown();
        }
        self.specs_cache.remove(name);
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

    pub async fn schemas(
        &self,
        disabled_plugins: &HashSet<String>,
        disabled_capabilities: &HashSet<(String, String, CapabilityFacet)>,
    ) -> Vec<ToolSchema> {
        let servers: Vec<(String, Arc<McpPlugin>)> = self
            .handlers
            .iter()
            .filter(|entry| !disabled_plugins.contains(entry.key()))
            .filter(|entry| entry.connection.health_status() == HealthStatus::Running)
            .map(|entry| (entry.key().clone(), Arc::clone(&entry.connection)))
            .collect();

        let disabled: HashSet<(&str, &str, CapabilityFacet)> = disabled_capabilities
            .iter()
            .map(|(name, tool, facet)| (name.as_str(), tool.as_str(), *facet))
            .collect();

        let mut schemas = Vec::new();
        for (name, connection) in servers {
            match self
                .specs_cache
                .specs(name.clone(), || async move { connection.specs().await })
                .await
            {
                Ok(specs) => schemas.extend(
                    specs
                        .values()
                        .filter(|spec| {
                            !disabled.contains(&(
                                spec.name.as_str(),
                                spec.tool.as_str(),
                                CapabilityFacet::Mcp,
                            ))
                        })
                        .map(|spec| spec.schema.clone()),
                ),
                Err(e) => error!("fail to refresh tool specs for {name}: {e}"),
            }
        }
        schemas
    }

    pub fn health_level(&self) -> HealthLevel {
        HealthLevel::combine(
            self.handlers
                .iter()
                .map(|entry| entry.connection.health_status().into()),
        )
    }

    async fn locate(&self, name: &str) -> Option<(Arc<McpPlugin>, ToolSpec)> {
        let servers: Vec<(String, Arc<McpPlugin>)> = self
            .handlers
            .iter()
            .map(|entry| (entry.key().clone(), Arc::clone(&entry.connection)))
            .collect();

        for (server, connection) in servers {
            if let Some(spec) = self.specs_cache.spec(&server, name).await {
                return Some((connection, spec));
            }
        }
        None
    }

    pub async fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.locate(name).await.map(|(_, spec)| spec)
    }

    pub async fn call(
        &self,
        name: String, // encoded mcp function call name
        session_id: Uuid,
        call_id: String,
        args: Value,
    ) -> Result<ToolResult> {
        let Some((connection, spec)) = self.locate(&name).await else {
            return Err(McpControllerError::UnknownTool(name));
        };
        let tool = spec.tool;

        let token = self.sessions.entry(session_id).or_default().clone();
        Ok(connection.call(tool, token, call_id, args).await?)
    }

    pub fn cancel(&self, session_id: Uuid) {
        if let Some((_, token)) = self.sessions.remove(&session_id) {
            token.cancel();
        }
    }
}

fn tool_infos(
    server: &str,
    specs: Vec<ToolSpec>,
    disabled: &HashSet<(String, String, CapabilityFacet)>,
) -> Vec<CapabilityInfo> {
    let disabled: HashSet<(&str, &str, CapabilityFacet)> = disabled
        .iter()
        .map(|(server, tool, facet)| (server.as_str(), tool.as_str(), *facet))
        .collect();
    specs
        .into_iter()
        .map(|spec| {
            let flag = disabled.contains(&(server, spec.tool.as_str(), CapabilityFacet::Mcp));
            CapabilityInfo {
                id: spec.tool,
                description: spec.schema.description,
                facets: vec![(CapabilityFacet::Mcp, flag)],
            }
        })
        .collect()
}

struct McpHandler {
    description: String,
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
