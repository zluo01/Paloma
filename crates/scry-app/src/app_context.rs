use std::sync::Arc;
use std::time::Duration;

use scry_capability::tools::process_manager::ProcessManager;
use scry_controller::remote::{
    RemoteQuery, SessionManager, SessionManagerClient, ToolController, TurnManager,
};
use scry_controller::{ConnectController, LocalQuery, ProviderController};
use scry_storage::db::Storage;
use scry_storage::session::SessionWriter;
use tokio::sync::broadcast;

/// Tray-driven actions delivered to the GTK side.
#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    OpenSettings,
    Quit,
}

/// Process-wide application state.
pub struct AppContext {
    pub connect: ConnectController,
    pub local_query: LocalQuery,
    pub remote_query: RemoteQuery,
    pub session_manager_client: SessionManagerClient,
    pub hotkey: broadcast::Sender<()>,
    pub tray_events: broadcast::Sender<TrayEvent>,
}

impl AppContext {
    pub async fn build() -> Result<Arc<Self>, AppError> {
        let db_path = scry_config::database_path();
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let storage = Storage::new(&db_path).await?;

        let (connect, remote_query, session_manager_client) = Self::init_llm(storage).await?;
        let local_query = Self::init_local()?;

        let (hotkey, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);
        let (tray_events, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);

        Ok(Arc::new(Self {
            connect,
            local_query,
            remote_query,
            session_manager_client,
            hotkey,
            tray_events,
        }))
    }

    async fn init_llm(
        storage: Storage,
    ) -> Result<(ConnectController, RemoteQuery, SessionManagerClient), AppError> {
        let session_path = scry_config::session_dir();
        tokio::fs::create_dir_all(&session_path).await?;

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

        let (mut session_writer, session_writer_client) = SessionWriter::new(session_path.clone());
        tokio::spawn(async move { session_writer.run().await });

        let (mut session_manager, session_manager_client) =
            SessionManager::new(session_path, &storage).await?;
        tokio::spawn(async move { session_manager.run(&session_writer_client).await });

        let (mut process_manager, process_manager_client) = ProcessManager::new();
        tokio::spawn(async move { process_manager.run().await });

        let tool_controller = Arc::new(ToolController::new(process_manager_client));

        let (mut turn_manager, turn_manager_client) = TurnManager::new(
            storage.clone(),
            Arc::clone(&provider_controller),
            session_manager_client.clone(),
            tool_controller,
        );
        tokio::spawn(async move { turn_manager.run().await });

        let remote_query =
            RemoteQuery::new(storage, session_manager_client.clone(), turn_manager_client);

        Ok((connect, remote_query, session_manager_client))
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
    Runtime(#[from] scry_controller::ProviderControllerError),

    #[error(transparent)]
    LocalQuery(#[from] scry_controller::LocalQueryInitError),
}
