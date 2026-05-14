use std::sync::Arc;
use std::time::Duration;

use scry_controller::remote_query::RemoteQuery;
use scry_controller::{ConnectController, LocalQuery, RuntimeController};
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
    pub hotkey: broadcast::Sender<()>,
    pub tray_events: broadcast::Sender<TrayEvent>,
}

impl AppContext {
    pub async fn build() -> Result<Arc<Self>, AppError> {
        let db_path = scry_config::database_path();
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let session_path = scry_config::session_dir();
        tokio::fs::create_dir_all(&session_path).await?;

        let storage = Storage::new(&db_path).await?;

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

        let runtime = Arc::new(RuntimeController::new(storage.clone(), http.clone()).await?);
        let connect = ConnectController::new(storage.clone(), Arc::clone(&runtime), http);

        let (mut writer, writer_tx) = SessionWriter::new(session_path);
        tokio::spawn(async move { writer.run().await });

        let remote_query = RemoteQuery::new(storage, runtime, writer_tx);
        let local_query = LocalQuery::new()?;

        let (hotkey, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);
        let (tray_events, _) = broadcast::channel(scry_config::HOTKEY_CHANNEL_CAPACITY);

        Ok(Arc::new(Self {
            connect,
            local_query,
            remote_query,
            hotkey,
            tray_events,
        }))
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
    Runtime(#[from] scry_controller::RuntimeControllerError),

    #[error(transparent)]
    LocalQuery(#[from] scry_controller::local_query::LocalQueryInitError),
}
