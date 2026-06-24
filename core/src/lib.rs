use std::{sync::Arc, time::Duration};

use futures::Stream;
use uuid::Uuid;

use crate::{
    capability::ProcessManager,
    constants::DATABASE_PATH,
    controller::{
        ConnectController, ConnectError, PermissionWorkflowManager, PluginConnectionController,
        PluginConnectionError, ProviderController, ProviderControllerError, RemoteQuery,
        RemoteQueryError, SearchQuery, SearchQueryInitError, SessionManager, SessionManagerError,
        ToolController, TurnManager,
    },
    db::{Storage, StorageError},
    permission::PermissionController,
};

mod capability;
mod constants;
mod controller;
mod db;
mod entity;
mod permission;
mod provider;
mod utils;

pub use capability::{Action, ActionOutcome, IconRef, Item};
pub use controller::{
    ChatRenderEvent, Connector, ConnectorConnection, McpServer, RenderEvent, SearchRenderEvent,
    SessionListItem,
};
pub use entity::{
    HealthLevel, HealthStatus, Plugin, PluginArgs, PluginType, ProviderId, Transport,
};
pub use permission::{PermissionState, UserDecision};
pub use provider::Connection;

pub struct AppContext {
    connect: ConnectController,
    search_query: SearchQuery,
    remote_query: RemoteQuery,
    plugin: PluginConnectionController,
}

impl AppContext {
    pub async fn build() -> Result<Arc<Self>> {
        let db_path = DATABASE_PATH.clone();
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let storage = Storage::new(&db_path).await?;

        let (connect, remote_query, plugin) = Self::init_llm(storage).await?;
        let search_query = Self::init_search()?;

        Ok(Arc::new(Self {
            connect,
            search_query,
            remote_query,
            plugin,
        }))
    }

    async fn init_llm(
        storage: Storage,
    ) -> Result<(ConnectController, RemoteQuery, PluginConnectionController)> {
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

        let provider_controller =
            Arc::new(ProviderController::new(storage.clone(), http.clone()).await?);
        let connect =
            ConnectController::new(storage.clone(), Arc::clone(&provider_controller), http);

        let permission_controller = PermissionController::new(storage.clone());
        let (mut permission_workflow_manager, permission_workflow_client) =
            PermissionWorkflowManager::new(permission_controller);
        tokio::spawn(async move { permission_workflow_manager.run().await });

        let (mut process_manager, process_manager_client) = ProcessManager::new();
        tokio::spawn(async move { process_manager.run().await });

        let tool_controller = Arc::new(
            ToolController::new(
                storage.clone(),
                process_manager_client,
                permission_workflow_client.clone(),
            )
            .await,
        );

        let plugin = PluginConnectionController::new(storage.clone(), Arc::clone(&tool_controller));

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
            Arc::clone(&provider_controller),
        );

        Ok((connect, remote_query, plugin))
    }

    fn init_search() -> Result<SearchQuery> {
        Ok(SearchQuery::new()?)
    }

    pub fn query(&self, input: &str) -> impl Stream<Item = RenderEvent> + use<> {
        self.search_query.query(input)
    }

    pub fn run_query_action(&self, id: &str, action: Action) -> Option<ActionOutcome> {
        self.search_query.run(id, action)
    }

    pub async fn init_chat(
        &self,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<(Uuid, bool)> {
        Ok(self
            .remote_query
            .init_chat(session_id, provider_id, prompt)
            .await?)
    }

    pub async fn chat(
        &self,
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
    ) -> Result<impl Stream<Item = RenderEvent> + use<>> {
        Ok(self
            .remote_query
            .chat(session_id, provider_id, prompt)
            .await?)
    }

    pub async fn cleanup_error_session(&self, session_id: Uuid) {
        self.remote_query.cleanup(session_id).await;
    }

    pub async fn available_sessions(&self) -> Result<Vec<SessionListItem>> {
        Ok(self.remote_query.available_sessions().await?)
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

    pub async fn init_connection(&self, provider_id: ProviderId) -> Result<Connection> {
        Ok(self.connect.init(provider_id).await?)
    }

    pub async fn finalize_connection(
        &self,
        provider_id: ProviderId,
        payload: Connection,
    ) -> Result<()> {
        Ok(self.connect.finalize(provider_id, payload).await?)
    }

    pub async fn disconnect_connector(&self, provider_id: ProviderId) -> Result<()> {
        Ok(self.connect.disconnect(provider_id).await?)
    }

    pub async fn set_model_preference(
        &self,
        provider_id: ProviderId,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        Ok(self
            .connect
            .set_preferred(provider_id, model, effort)
            .await?)
    }

    pub async fn prefer_model(&self) -> Result<Option<ProviderId>> {
        Ok(self.connect.prefer_provider().await?)
    }

    pub async fn available_connectors(&self) -> Result<Vec<Connector>> {
        Ok(self.connect.available_connectors().await?)
    }

    pub async fn connectors_health_level(&self) -> HealthLevel {
        self.connect.health_level().await
    }

    pub async fn plugins_health_level(&self) -> HealthLevel {
        self.plugin.health_level().await
    }

    pub async fn list_mcps(&self) -> Result<Vec<McpServer>> {
        Ok(self.plugin.list_mcps().await?)
    }

    pub async fn add_mcp(&self, plugin: Plugin) -> Result<()> {
        Ok(self.plugin.add_mcp(plugin).await?)
    }

    pub async fn update_plugin(&self, plugin_type: PluginType, plugin: Plugin) -> Result<()> {
        Ok(self.plugin.update_plugin(plugin_type, plugin).await?)
    }

    pub async fn remove_plugin(&self, plugin_type: PluginType, name: &str) -> Result<()> {
        Ok(self.plugin.remove_plugin(plugin_type, name).await?)
    }

    pub async fn toggle_plugin(&self, name: &str, disabled: bool) -> Result<()> {
        Ok(self.plugin.toggle_plugin(name, disabled).await?)
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
    Provider(#[from] ProviderControllerError),

    #[error(transparent)]
    RemoteQuery(#[from] RemoteQueryError),

    #[error(transparent)]
    Connect(#[from] ConnectError),

    #[error(transparent)]
    PluginConnection(#[from] PluginConnectionError),

    #[error(transparent)]
    SearchQuery(#[from] SearchQueryInitError),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),
}

type Result<T> = std::result::Result<T, AppError>;
