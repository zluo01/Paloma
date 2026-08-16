use paloma_core::AppError;

/// Core failure crossing the FFI boundary. The frontend only ever cares
/// whether a call failed and what the message is, so the whole [`AppError`]
/// tree flattens into a single message-carrying case.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PalomaError {
    #[error("{message}")]
    Failure { message: String },
}

impl PalomaError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::Failure {
            message: message.into(),
        }
    }
}

impl From<AppError> for PalomaError {
    fn from(err: AppError) -> Self {
        Self::new(err.to_string())
    }
}
