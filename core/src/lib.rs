use std::{path::PathBuf, sync::Arc, time::Duration};

use futures::Stream;
use log::error;
use uuid::Uuid;

use crate::{
    controller::{
        ExtensionController, ExtensionControllerError, PermissionWorkflowManager,
        ProviderController, ProviderControllerError, RemoteQuery, RemoteQueryError, SessionManager,
        SessionManagerError, ToolController, TurnManager,
    },
    db::{Storage, StorageError},
    permission::PermissionController,
};

mod capability;
mod constants;
mod controller;
mod db;
mod entity;
mod extension;
mod mcp;
mod permission;
mod provider;
mod utils;

#[ctor::ctor(unsafe)]
fn process_entry() {
    provider::serve_plugin_and_exit_if_requested();
    extension::serve_extension_plugin_and_exit_if_requested();
}

pub use constants::RENDER_CHANNEL_CAPACITY;
pub use controller::{
    ChatRenderEvent, Connector, ConnectorConnection, ProviderStatus, QueryResponse, RenderEvent,
    SearchRenderEvent, SessionListItem,
};
pub use db::Permission;
pub use entity::{
    ExtensionCapabilityId, HealthLevel, HealthStatus, Icon, Plugin, PluginArgs, PluginType,
    ProviderBackendId, Transport,
};
pub use extension::ExtensionInfo;
pub use mcp::McpPluginInfo;
pub use permission::{PermissionState, UserDecision};
pub use provider::ProviderInfo;
pub use scry_extension_protocol::v1::{
    Action, CapabilityIcon, Item, capability_icon, run_action_response::Behavior,
};
pub use scry_provider_protocol::v1::{
    BrowserRedirect, ConnectionPayload, DeviceCode, ManualInput, Model, ProviderAuthMethod,
    connection_payload,
};
pub use utils::OAuthCallbackState;

use crate::{
    constants::{APP_NAME, DATABASE_FILE},
    controller::{ChatRenderStream, McpController, McpControllerError},
};

pub struct AppContext {
    storage: Storage,
    remote_query: RemoteQuery,
    providers: Arc<ProviderController>,
    extensions: Arc<ExtensionController>,
    mcps: Arc<McpController>,
}

impl AppContext {
    /// `app_data_path` is the platform data parent, such as `~/.local/share`
    /// or `~/Library/Application Support`.
    pub async fn build(app_data_path: PathBuf) -> Result<Arc<Self>> {
        let db_path = app_data_path.join(APP_NAME).join(DATABASE_FILE);
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let storage = Storage::new(&db_path).await?;

        let (providers, extensions, mcps, remote_query) = Self::init(storage.clone()).await?;

        Ok(Arc::new(Self {
            storage,
            providers,
            extensions,
            mcps,
            remote_query,
        }))
    }

    async fn init(
        storage: Storage,
    ) -> Result<(
        Arc<ProviderController>,
        Arc<ExtensionController>,
        Arc<McpController>,
        RemoteQuery,
    )> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(900))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(5)
            .http2_adaptive_window(true)
            .http2_keep_alive_interval(Some(Duration::from_secs(60)))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)
            .build()?;

        let mcp_controller = Arc::new(McpController::new(storage.clone(), http.clone()).await?);

        let provider_controller = Arc::new(ProviderController::new(storage.clone()).await?);

        let extension_controller = Arc::new(ExtensionController::new(storage.clone()).await?);

        let permission_controller = PermissionController::new(storage.clone());
        let (mut permission_workflow_manager, permission_workflow_client) =
            PermissionWorkflowManager::new(permission_controller);
        tokio::spawn(async move { permission_workflow_manager.run().await });

        let tool_controller = ToolController::new(
            storage.clone(),
            Arc::clone(&mcp_controller),
            permission_workflow_client.clone(),
        )
        .await;

        let (mut session_manager, session_manager_client) = SessionManager::new(
            storage.clone(),
            tool_controller.clone(),
            permission_workflow_client.clone(),
        )
        .await?;
        tokio::spawn(async move { session_manager.run().await });

        let (mut turn_manager, turn_manager_client) = TurnManager::new(
            storage,
            Arc::clone(&provider_controller),
            session_manager_client.clone(),
            permission_workflow_client.clone(),
            tool_controller,
        );
        tokio::spawn(async move { turn_manager.run().await });

        let remote_query = RemoteQuery::new(
            session_manager_client,
            turn_manager_client,
            permission_workflow_client,
        );

        Ok((
            provider_controller,
            extension_controller,
            mcp_controller,
            remote_query,
        ))
    }

    pub fn search(&self, input: &str) -> impl Stream<Item = RenderEvent> + use<> {
        self.extensions.search(input)
    }

    pub async fn run_search_action(
        &self,
        id: ExtensionCapabilityId,
        action: Action,
    ) -> Result<Behavior> {
        Ok(self.extensions.run_search_action(id, action).await?)
    }

    pub async fn chat(
        &self,
        session_id: Option<Uuid>,
        provider_backend_id: ProviderBackendId,
        prompt: String,
    ) -> ChatRenderStream {
        self.remote_query
            .chat(session_id, provider_backend_id, prompt)
            .await
    }

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>> {
        Ok(self.remote_query.available_sessions().await?)
    }

    pub async fn search_sessions(&self, needle: String) -> Result<Vec<Uuid>> {
        let ids = self.storage.search_sessions(&needle).await?;
        Ok(ids
            .iter()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect())
    }

    pub async fn restore_session(
        &self,
        session_id: Uuid,
    ) -> Result<impl Stream<Item = RenderEvent> + use<>> {
        Ok(self.remote_query.restore_session(session_id).await?)
    }

    pub async fn remove_session(&self, session_id: Uuid) -> Result<()> {
        Ok(self.remote_query.remove_session(session_id).await?)
    }

    pub async fn cancel_session(&self, session_id: Uuid) -> Result<()> {
        Ok(self.remote_query.cancel(session_id).await?)
    }

    pub async fn decide_toolcall_permissions(
        &self,
        user_decision: UserDecision,
    ) -> Result<PermissionState> {
        Ok(self.remote_query.decide(user_decision).await?)
    }

    pub async fn get_permissions(&self) -> Result<Vec<Permission>> {
        Ok(self.storage.get_permissions().await?)
    }

    pub async fn delete_permission(&self, prefix: &str) -> Result<()> {
        Ok(self.storage.delete_permission(prefix).await?)
    }
}

/// model connections + config
impl AppContext {
    pub async fn init_connection(
        &self,
        provider_backend_id: ProviderBackendId,
    ) -> Result<ConnectionPayload> {
        Ok(self.providers.init(provider_backend_id).await?)
    }

    pub async fn finalize_connection(
        &self,
        provider_auth_method: ProviderAuthMethod,
        provider_backend_id: ProviderBackendId,
        payload: String,
    ) -> Result<()> {
        Ok(self
            .providers
            .finalize(provider_auth_method, provider_backend_id, payload)
            .await?)
    }

    pub async fn cancel_connection(&self, provider_backend_id: ProviderBackendId) -> Result<()> {
        Ok(self
            .providers
            .cancel_connection(provider_backend_id)
            .await?)
    }

    pub async fn disconnect_connector(&self, provider_backend_id: ProviderBackendId) -> Result<()> {
        Ok(self.providers.disconnect(provider_backend_id).await?)
    }

    pub async fn set_model_preference(
        &self,
        provider_backend_id: ProviderBackendId,
        model: &str,
        effort: &str,
        as_default: bool,
    ) -> Result<()> {
        Ok(self
            .providers
            .set_preferred(provider_backend_id, model, effort, as_default)
            .await?)
    }

    pub async fn prefer_model(&self) -> Result<Option<ProviderBackendId>> {
        Ok(self.providers.prefer_provider().await?)
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>> {
        Ok(self.providers.available_connectors().await?)
    }

    pub async fn connectors_health_level(&self) -> HealthLevel {
        self.providers.health_level().await
    }
}

/// plugins + mcps
impl AppContext {
    pub async fn plugins_health_level(&self) -> HealthLevel {
        let servers = match self.mcps.list_mcps().await {
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

    pub async fn list_extension_plugins(&self) -> Result<Vec<ExtensionInfo>> {
        Ok(self.extensions.available_extensions().await?)
    }

    pub async fn list_provider_plugins(&self) -> Result<Vec<ProviderInfo>> {
        Ok(self.providers.available_providers().await?)
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpPluginInfo>> {
        Ok(self.mcps.list_mcps().await?)
    }

    pub async fn add_extension_plugin(&self, config: Plugin) -> Result<()> {
        Ok(self.extensions.add_extension(&config).await?)
    }

    pub async fn add_provider_plugin(&self, config: Plugin) -> Result<()> {
        Ok(self.providers.add_provider(&config).await?)
    }

    pub async fn init_mcp_connection(&self, config: Plugin) -> Result<Option<OAuthCallbackState>> {
        Ok(self.mcps.init_connection(config).await?)
    }

    pub async fn finalize_mcp_connection(
        &self,
        config: Plugin,
        state: Option<OAuthCallbackState>,
    ) -> Result<()> {
        Ok(self.mcps.finalize_connection(config, state).await?)
    }

    pub async fn update_plugin(&self, plugin_type: PluginType, plugin: Plugin) -> Result<()> {
        match plugin_type {
            PluginType::Extension => self.extensions.update_extension(&plugin).await?,
            PluginType::Mcp => self.mcps.update_mcp(&plugin).await?,
            PluginType::Provider => self.providers.update_provider(&plugin).await?,
        }
        Ok(())
    }

    pub async fn remove_plugin(&self, plugin_type: PluginType, name: &str) -> Result<()> {
        match plugin_type {
            PluginType::Extension => self.extensions.remove_extension(name).await?,
            PluginType::Mcp => self.mcps.remove_mcp(name).await?,
            PluginType::Provider => self.providers.remove_provider(name).await?,
        }
        Ok(())
    }

    pub async fn toggle_plugin(&self, name: &str, disabled: bool) -> Result<()> {
        self.storage.toggle_plugin(name, disabled).await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Mcp(#[from] McpControllerError),

    #[error(transparent)]
    Provider(#[from] ProviderControllerError),

    #[error(transparent)]
    Extension(#[from] ExtensionControllerError),

    #[error(transparent)]
    RemoteQuery(#[from] RemoteQueryError),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),
}

type Result<T> = std::result::Result<T, AppError>;
