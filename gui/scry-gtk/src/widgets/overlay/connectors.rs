//! Shared predicates over [`Connector`] state.

use scry_core::{Connector, HealthStatus};

/// The provider's runtime is connected and healthy.
pub(super) fn is_running(connector: &Connector) -> bool {
    connector
        .connection
        .as_ref()
        .is_some_and(|c| c.status.status == HealthStatus::Running)
}

/// The provider is marked the user's preferred one.
pub(super) fn is_preferred(connector: &Connector) -> bool {
    connector.connection.as_ref().is_some_and(|c| c.preferred)
}
