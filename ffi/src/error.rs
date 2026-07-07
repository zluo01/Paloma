use scry_core::AppError;

/// Mirror of [`scry_core::AppError`] flattened to message-only variants so it
/// crosses the FFI boundary as a typed Swift error.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ScryError {
    #[error("{message}")]
    Io { message: String },

    #[error("{message}")]
    Storage { message: String },

    #[error("{message}")]
    Http { message: String },

    #[error("{message}")]
    Provider { message: String },

    #[error("{message}")]
    RemoteQuery { message: String },

    #[error("{message}")]
    Connect { message: String },

    #[error("{message}")]
    PluginConnection { message: String },

    #[error("{message}")]
    SearchQuery { message: String },

    #[error("{message}")]
    SessionManager { message: String },

    /// A value handed in from the foreign side could not be converted back
    /// into its core representation (bad JSON payload, reused OAuth handle).
    #[error("{message}")]
    InvalidArgument { message: String },

    /// FFI-layer failure: the embedded Tokio runtime rejected or aborted the
    /// task running the core call.
    #[error("{message}")]
    Runtime { message: String },
}

impl From<AppError> for ScryError {
    fn from(err: AppError) -> Self {
        let message = err.to_string();
        match err {
            AppError::Io(_) => Self::Io { message },
            AppError::Storage(_) => Self::Storage { message },
            AppError::Reqwest(_) => Self::Http { message },
            AppError::Provider(_) => Self::Provider { message },
            AppError::RemoteQuery(_) => Self::RemoteQuery { message },
            AppError::Connect(_) => Self::Connect { message },
            AppError::PluginConnection(_) => Self::PluginConnection { message },
            AppError::SearchQuery(_) => Self::SearchQuery { message },
            AppError::SessionManager(_) => Self::SessionManager { message },
        }
    }
}
