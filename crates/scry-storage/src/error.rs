#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("provider {0} not found")]
    NotFound(String),

    #[error("provider {0} already exists")]
    Duplicate(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
