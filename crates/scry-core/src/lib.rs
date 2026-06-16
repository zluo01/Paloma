use std::{sync::Arc, time::Duration};

use scry_capability::ProcessManager;
pub use scry_capability::{Action, ActionOutcome, HealthStatus, IconRef, Item};
pub use scry_config::RENDER_CHANNEL_CAPACITY;
pub use scry_controller::{
    ChatRenderEvent, Connection, Connector, ConnectorConnection, HealthLevel, LocalRenderEvent,
    McpServer, PermissionState, Plugin, PluginArgs, PluginType, ProviderHealthStatus, ProviderId,
    RenderEvent, SessionUpdate, TerminalState, Transport, UserDecision,
};
use scry_controller::{
    ConnectController, LocalQuery, PermissionWorkflowManager, PluginConnectionController,
    ProviderController, RemoteQuery, SessionManager, ToolController, TurnManager,
};
use scry_permission::PermissionController;
use scry_storage::Storage;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    OpenSettings,
    Quit,
}

pub struct AppContext {
    pub connect: ConnectController,
    pub local_query: LocalQuery,
    pub remote_query: RemoteQuery,
    pub plugin: PluginConnectionController,
    pub hotkey: broadcast::Sender<()>,
    pub tray_events: broadcast::Sender<TrayEvent>,
}

impl AppContext {
    pub async fn build() -> Result<Arc<Self>, AppError> {
        let db_path = scry_config::DATABASE_PATH.clone();
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let storage = Storage::new(&db_path).await?;

        let (connect, remote_query, plugin) = Self::init_llm(storage).await?;
        let local_query = Self::init_local()?;

        let (hotkey, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);
        let (tray_events, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);

        Ok(Arc::new(Self {
            connect,
            local_query,
            remote_query,
            plugin,
            hotkey,
            tray_events,
        }))
    }

    async fn init_llm(
        storage: Storage,
    ) -> Result<(ConnectController, RemoteQuery, PluginConnectionController), AppError> {
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

    fn init_local() -> Result<LocalQuery, AppError> {
        Ok(LocalQuery::new()?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Storage(#[from] scry_storage::StorageError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Provider(#[from] scry_controller::ProviderControllerError),

    #[error(transparent)]
    LocalQuery(#[from] scry_controller::LocalQueryInitError),

    #[error(transparent)]
    SessionManager(#[from] scry_controller::SessionManagerError),
}
