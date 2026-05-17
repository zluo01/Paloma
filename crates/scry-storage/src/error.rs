#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("provider {0} not found")]
    NotFound(String),

    #[error("provider {0} already exists")]
    Duplicate(String),

    #[error("session writer channel closed")]
    ChannelClosed,
}

pub type Result<T> = std::result::Result<T, StorageError>;
