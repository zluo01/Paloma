mod connector;
mod entity;
mod helper;
mod local;
mod remote;

pub use connector::{
    ConnectController, ConnectError, Connector, ConnectorConnection, McpServer,
    PluginConnectionController, PluginConnectionError,
};
pub use entity::{ChatRenderEvent, HealthLevel, LocalRenderEvent, RenderEvent};
pub use local::{LocalQuery, LocalQueryInitError};
pub use remote::{
    PermissionState, PermissionWorkflowError, PermissionWorkflowManager, ProviderController,
    ProviderControllerError, RemoteQuery, RemoteQueryError, SessionManager, SessionManagerError,
    SessionUpdate, TerminalState, ToolController, ToolControllerError, ToolStatus, TurnManager,
    UserDecision,
};
// Storage entity types that appear in this crate's public API, re-exported
// so UI crates don't need a direct scry-storage dependency.
pub use scry_storage::{Plugin, PluginArgs, PluginType, Transport};
