use std::sync::Arc;

use dashmap::DashMap;
use futures::{Stream, future::join_all, stream};
use log::error;
use scry_extension_protocol::v1::{
    Action, Capability, Facet, HandshakeResponse, run_action_response::Behavior,
};
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    HealthLevel, HealthStatus, Plugin, PluginType, QueryResponse, RENDER_CHANNEL_CAPACITY,
    RenderEvent, SearchRenderEvent, Transport,
    db::{Storage, StorageError},
    entity::ExtensionCapabilityId,
    extension::{ExtensionConnectionError, ExtensionInfo, ExtensionPlugin, INTERNAL_PLUGIN},
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
            [&*INTERNAL_PLUGIN]
                .into_iter()
                .chain(extension_plugins.iter())
                .map(|plugin| async move {
                    (plugin.name.as_str(), init_extension_plugin(plugin).await)
                }),
        )
        .await;

        let handlers: DashMap<String, ExtensionHandler> = DashMap::new();
        for (name, result) in results {
            match result {
                Ok((detail, connection)) => {
                    handlers.insert(
                        detail.extension_id,
                        ExtensionHandler {
                            description: detail.description,
                            author: detail.author,
                            homepage: detail.homepage,
                            capabilities: detail.capabilities,
                            connection,
                        },
                    );
                },
                Err(e) => error!("failed to initialize extension plugin {name}: {e}"),
            }
        }

        Ok(Self { handlers, storage })
    }

    pub(crate) fn search(&self, input: &str) -> impl Stream<Item = RenderEvent> + use<> {
        let (render_tx, mut render_rx) = mpsc::channel(RENDER_CHANNEL_CAPACITY);

        // every capability whose facet allows search, with its connection
        let handlers: Vec<(ExtensionCapabilityId, Arc<ExtensionPlugin>)> = self
            .handlers
            .iter()
            .flat_map(|entry| {
                let handler = entry.value();
                handler
                    .capabilities
                    .iter()
                    .filter(|capability| matches!(capability.facet(), Facet::Query | Facet::Both))
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
            if input.trim().is_empty() {
                let _ = render_tx.send(RenderEvent::Done).await;
                return;
            }

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

        stream::poll_fn(move |cx| render_rx.poll_recv(cx))
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
        Ok([&*INTERNAL_PLUGIN]
            .into_iter()
            .map(|builtin| (builtin.name.clone(), None))
            .chain(
                plugins
                    .into_iter()
                    .map(|config| (config.name.clone(), Some(config))),
            )
            .filter_map(|(name, config)| {
                let Some(handler) = self.handlers.get(&name) else {
                    error!(
                        "no live extension status for plugin {name}; This indicates a bug. Skipping"
                    );
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
        let (detail, provider) = init_extension_plugin(plugin).await?;

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

        self.handlers.insert(
            detail.extension_id,
            ExtensionHandler {
                description: detail.description,
                author: detail.author,
                homepage: detail.homepage,
                capabilities: detail.capabilities,
                connection: provider,
            },
        );
        Ok(())
    }

    pub async fn update_extension(&self, plugin: &Plugin) -> Result<()> {
        let (detail, provider) = init_extension_plugin(plugin).await?;

        if provider.health() != HealthStatus::Running {
            return Err(ExtensionControllerError::FailToInitialize(
                detail.extension_id,
            ));
        }

        self.storage
            .update_plugin(
                &detail.extension_id,
                plugin.transport,
                plugin.timeout,
                &plugin.env,
                &plugin.args,
            )
            .await?;

        self.handlers.insert(
            detail.extension_id,
            ExtensionHandler {
                description: detail.description,
                author: detail.author,
                homepage: detail.homepage,
                capabilities: detail.capabilities,
                connection: provider,
            },
        );
        Ok(())
    }

    pub async fn remove_extension(&self, extension_id: &str) -> Result<()> {
        self.storage.delete_plugin(extension_id).await?;
        if let Some((_, handler)) = self.handlers.remove(extension_id) {
            handler.connection.shutdown();
        }
        Ok(())
    }
}

struct ExtensionHandler {
    description: String,
    author: Option<String>,
    homepage: Option<String>,
    capabilities: Vec<Capability>,
    connection: Arc<ExtensionPlugin>,
}

async fn init_extension_plugin(
    plugin: &Plugin,
) -> Result<(HandshakeResponse, Arc<ExtensionPlugin>)> {
    let provider = ExtensionPlugin::connect(plugin)?;
    let provider_detail = provider.handshake().await?;
    Ok((provider_detail, provider))
}

#[derive(Debug, thiserror::Error)]
pub enum ExtensionControllerError {
    #[error("extension plugin {0} is not registered")]
    UnknownExtension(String),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Connection(#[from] ExtensionConnectionError),

    #[error("fail to initialize extension plugin: {0}")]
    FailToInitialize(String),
}

type Result<T> = std::result::Result<T, ExtensionControllerError>;
