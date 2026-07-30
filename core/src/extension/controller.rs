use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dashmap::DashMap;
use futures::{Stream, future::join_all, stream};
use log::{error, warn};
use scry_extension_protocol::v1::{
    Action, Capability, HandshakeResponse, run_action_response::Behavior,
};
use serde_json::Value;
use tokio::{sync::mpsc, task::JoinSet};
use uuid::Uuid;

use super::{BUILTIN_EXTENSIONS, ExtensionConnectionError, ExtensionInfo, ExtensionPlugin};
use crate::{
    HealthLevel, HealthStatus, Plugin, PluginType, QueryResponse, RENDER_CHANNEL_CAPACITY,
    RenderEvent, SearchRenderEvent, Transport,
    db::{Storage, StorageError},
    entity::{ExtensionCapabilityId, ToolResult, ToolSchema, ToolSpec},
    utils::ext_tool_name_encode,
};

pub struct ExtensionController {
    handlers: DashMap<String, ExtensionHandler>,
    storage: Storage,
}

impl ExtensionController {
    pub async fn new(storage: Storage) -> Result<Self> {
        let extension_plugins = storage.plugins_by_type(PluginType::Extension).await?;

        // init in parallel
        let results = join_all(
            BUILTIN_EXTENSIONS
                .iter()
                .chain(extension_plugins.iter())
                .map(|plugin| async move {
                    (plugin.name.as_str(), init_extension_plugin(plugin).await)
                }),
        )
        .await;

        let handlers: DashMap<String, ExtensionHandler> = DashMap::new();
        for (index, (name, result)) in results.into_iter().enumerate() {
            match result {
                Ok((detail, connection)) => {
                    let specs = capability_specs(&detail.extension_id, &detail.capabilities);
                    handlers.insert(
                        detail.extension_id,
                        ExtensionHandler {
                            description: detail.description,
                            author: detail.author,
                            homepage: detail.homepage,
                            capabilities: detail.capabilities,
                            specs,
                            connection,
                        },
                    );
                },
                Err(e) if index < BUILTIN_EXTENSIONS.len() => {
                    return Err(ExtensionControllerError::FailToInitialize(format!(
                        "{name}: {e}"
                    )));
                },
                Err(e) => {
                    error!("failed to initialize extension plugin {name}: {e}");
                    handlers.insert(
                        name.to_string(),
                        ExtensionHandler {
                            description: String::new(),
                            author: None,
                            homepage: None,
                            capabilities: Vec::new(),
                            specs: HashMap::new(),
                            connection: ExtensionPlugin::unhealthy(e.to_string()),
                        },
                    );
                },
            }
        }

        Ok(Self { handlers, storage })
    }

    pub(crate) async fn search(&self, input: &str) -> impl Stream<Item = RenderEvent> + use<> {
        let (render_tx, mut render_rx) = mpsc::channel(RENDER_CHANNEL_CAPACITY);
        let stream = stream::poll_fn(move |cx| render_rx.poll_recv(cx));

        if input.trim().is_empty() {
            let _ = render_tx.send(RenderEvent::Done).await;
            return stream;
        }

        let disabled = match self.storage.disabled_plugins().await {
            Ok(disabled) => disabled,
            Err(e) => {
                let _ = render_tx
                    .send(RenderEvent::Error {
                        message: format!("fail to get disabled plugins: {e}"),
                    })
                    .await;
                // the GUI resolves a query only on Done
                let _ = render_tx.send(RenderEvent::Done).await;
                return stream;
            },
        };

        // every enabled capability whose facet allows search, with its connection
        let handlers: Vec<(ExtensionCapabilityId, Arc<ExtensionPlugin>)> = self
            .handlers
            .iter()
            .filter(|entry| !disabled.contains(entry.key()))
            .filter(|entry| entry.connection.health() == HealthStatus::Running)
            .flat_map(|entry| {
                let handler = entry.value();
                handler
                    .capabilities
                    .iter()
                    .filter(|capability| capability.search.is_some())
                    .map(|capability| {
                        (
                            ExtensionCapabilityId {
                                extension_id: entry.key().clone(),
                                capability_id: capability.capability_id.clone(),
                            },
                            Arc::clone(&handler.connection),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let input = input.to_owned();

        tokio::spawn(async move {
            let mut set = JoinSet::new();
            for (extension_capability_id, extension) in handlers {
                let input = input.clone();
                set.spawn(async move {
                    extension
                        .search(extension_capability_id.capability_id.clone(), input)
                        .await
                        .map(|items| QueryResponse {
                            extension_capability_id: extension_capability_id.clone(),
                            name: extension_capability_id.capability_id,
                            items,
                        })
                });
            }

            while let Some(joined) = set.join_next().await {
                let event = match joined {
                    Ok(Ok(response)) => RenderEvent::Search(SearchRenderEvent::Append { response }),
                    Ok(Err(err)) => RenderEvent::Error {
                        message: err.to_string(),
                    },
                    Err(err) => RenderEvent::Error {
                        message: err.to_string(),
                    },
                };
                if render_tx.send(event).await.is_err() {
                    error!("failed to send render response.");
                    return;
                }
            }

            if render_tx.send(RenderEvent::Done).await.is_err() {
                error!("failed to send done event.");
            }
        });

        stream
    }

    pub async fn run_search_action(
        &self,
        extension_capability_id: ExtensionCapabilityId,
        action: Action,
    ) -> Result<Behavior> {
        let connection = self
            .handlers
            .get(&extension_capability_id.extension_id)
            .map(|handler| Arc::clone(&handler.connection))
            .ok_or_else(|| {
                ExtensionControllerError::UnknownExtension(
                    extension_capability_id.extension_id.to_string(),
                )
            })?;

        Ok(connection
            .run_search_action(extension_capability_id.capability_id.to_string(), action)
            .await?)
    }

    pub fn health_level(&self) -> HealthLevel {
        HealthLevel::combine(
            self.handlers
                .iter()
                .map(|entry| entry.connection.health().into()),
        )
    }

    pub async fn available_extensions(&self) -> Result<Vec<ExtensionInfo>> {
        let plugins = self.storage.plugins_by_type(PluginType::Extension).await?;
        Ok(BUILTIN_EXTENSIONS
            .iter()
            .map(|builtin| (builtin.name.clone(), None))
            .chain(
                plugins
                    .into_iter()
                    .map(|config| (config.name.clone(), Some(config))),
            )
            .filter_map(|(name, config)| {
                let Some(handler) = self.handlers.get(&name) else {
                    error!("no extension handler for plugin {name}; This indicates a bug.");
                    return None;
                };
                Some(ExtensionInfo {
                    name,
                    description: handler.description.clone(),
                    author: handler.author.clone(),
                    homepage: handler.homepage.clone(),
                    capabilities: handler.capabilities.clone(),
                    status: handler.connection.health(),
                    error: handler.connection.plugin_error(),
                    config,
                })
            })
            .collect())
    }

    pub async fn add_extension(&self, plugin: &Plugin) -> Result<()> {
        let (detail, connection) = init_extension_plugin(plugin).await?;

        self.storage
            .insert_plugin(
                &detail.extension_id,
                PluginType::Extension,
                Transport::Local,
                plugin.timeout,
                &plugin.env,
                &plugin.args,
                None,
            )
            .await?;

        self.register_handler(detail, connection);
        Ok(())
    }

    pub async fn update_extension(&self, plugin: &Plugin) -> Result<()> {
        let (detail, connection) = init_extension_plugin(plugin).await?;

        self.storage
            .update_plugin(
                &detail.extension_id,
                plugin.transport,
                plugin.timeout,
                &plugin.env,
                &plugin.args,
            )
            .await?;

        if let Some(previous) = self.register_handler(detail, connection) {
            previous.connection.shutdown();
        }
        Ok(())
    }

    fn register_handler(
        &self,
        detail: HandshakeResponse,
        connection: Arc<ExtensionPlugin>,
    ) -> Option<ExtensionHandler> {
        let specs = capability_specs(&detail.extension_id, &detail.capabilities);
        self.handlers.insert(
            detail.extension_id,
            ExtensionHandler {
                description: detail.description,
                author: detail.author,
                homepage: detail.homepage,
                capabilities: detail.capabilities,
                specs,
                connection,
            },
        )
    }

    pub async fn remove_extension(&self, extension_id: &str) -> Result<()> {
        self.storage.delete_plugin(extension_id).await?;
        if let Some((_, handler)) = self.handlers.remove(extension_id) {
            handler.connection.shutdown();
        }
        Ok(())
    }
}

impl ExtensionController {
    pub async fn invoke(
        &self,
        name: String, // encoded extension function call name
        session_id: Uuid,
        call_id: String,
        arguments: Value,
    ) -> Result<ToolResult> {
        let Some((connection, tool)) = self.handlers.iter().find_map(|entry| {
            let spec = entry.specs.get(&name)?;
            Some((Arc::clone(&entry.connection), spec.tool.clone()))
        }) else {
            return Err(ExtensionControllerError::UnknownTool(name));
        };

        Ok(connection
            .invoke_tool(tool, session_id.to_string(), call_id, arguments.to_string())
            .await?)
    }

    pub async fn cancel(&self, session_id: Uuid) {
        let tools: Vec<Arc<ExtensionPlugin>> = self
            .handlers
            .iter()
            .filter(|entry| entry.connection.health() == HealthStatus::Running)
            .filter(|entry| {
                entry
                    .capabilities
                    .iter()
                    .any(|capability| capability.tool.is_some())
            })
            .map(|entry| Arc::clone(&entry.connection))
            .collect();

        let results =
            join_all(tools.into_iter().map(|connection| async move {
                connection.cancel_tool(session_id.to_string()).await
            }))
            .await;
        for result in results {
            if let Err(e) = result {
                warn!("failed to cancel extension tool session {session_id}: {e}");
            }
        }
    }

    pub fn schemas(&self, disabled: &HashSet<String>) -> Vec<ToolSchema> {
        self.handlers
            .iter()
            .filter(|entry| !disabled.contains(entry.key()))
            .filter(|entry| entry.connection.health() == HealthStatus::Running)
            .flat_map(|entry| {
                entry
                    .specs
                    .values()
                    .map(|spec| spec.schema.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.handlers
            .iter()
            .find_map(|entry| entry.specs.get(name).cloned())
    }
}

fn capability_specs(extension_id: &str, capabilities: &[Capability]) -> HashMap<String, ToolSpec> {
    capabilities
        .iter()
        .filter_map(|capability| {
            let tool = capability.tool.as_ref()?;
            let parameters = match serde_json::from_str(&tool.parameters) {
                Ok(parameters) => parameters,
                Err(e) => {
                    error!(
                        "invalid tool parameters schema for capability {}: {e}",
                        capability.capability_id
                    );
                    return None;
                },
            };
            let spec = ToolSpec {
                name: extension_id.to_string(),
                tool: capability.capability_id.clone(),
                schema: ToolSchema {
                    name: ext_tool_name_encode(extension_id, &capability.capability_id),
                    description: tool.description.clone(),
                    parameters,
                },
            };
            Some((spec.schema.name.clone(), spec))
        })
        .collect()
}

struct ExtensionHandler {
    description: String,
    author: Option<String>,
    homepage: Option<String>,
    capabilities: Vec<Capability>,
    specs: HashMap<String, ToolSpec>,
    connection: Arc<ExtensionPlugin>,
}

async fn init_extension_plugin(
    plugin: &Plugin,
) -> Result<(HandshakeResponse, Arc<ExtensionPlugin>)> {
    let connection = ExtensionPlugin::connect(plugin).await?;
    let detail = connection.handshake().await?;
    Ok((detail, connection))
}

#[derive(Debug, thiserror::Error)]
pub enum ExtensionControllerError {
    #[error("extension plugin {0} is not registered")]
    UnknownExtension(String),

    #[error("unknown extension tool: {0}")]
    UnknownTool(String),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Connection(#[from] ExtensionConnectionError),

    #[error("fail to initialize builtin extension {0}")]
    FailToInitialize(String),
}

type Result<T> = std::result::Result<T, ExtensionControllerError>;
