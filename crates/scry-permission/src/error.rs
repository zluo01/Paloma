#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("empty command error.")]
    EmptyCommand,
    #[error(transparent)]
    Storage(#[from] scry_storage::StorageError),
}

pub type Result<T> = std::result::Result<T, PermissionError>;
