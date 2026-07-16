pub const INIT_TABLE_QUERY: &str = include_str!("queries/init_table.sql");

pub const INSERT_BACKEND_QUERY: &str = include_str!("queries/backends/insert_backend.sql");

pub const UPDATE_BACKEND_QUERY: &str = include_str!("queries/backends/update_backend.sql");

pub const UPDATE_BACKEND_PREFERENCES_QUERY: &str =
    include_str!("queries/backends/update_backend_preferences.sql");

pub const DELETE_BACKEND_QUERY: &str = include_str!("queries/backends/delete_backend.sql");

pub const CONNECTED_BACKENDS_QUERY: &str = include_str!("queries/backends/connected_backends.sql");

pub const PREFERRED_BACKEND_QUERY: &str = include_str!("queries/backends/preferred_backend.sql");

pub const CREATE_NEW_SESSION_QUERY: &str = include_str!("queries/sessions/create_new_session.sql");

pub const GET_ALL_SESSIONS_QUERY: &str = include_str!("queries/sessions/get_all_sessions.sql");

pub const SEARCH_SESSIONS_QUERY: &str = include_str!("queries/sessions/search_sessions.sql");

pub const DELETE_SESSION_QUERY: &str = include_str!("queries/sessions/delete_session.sql");

pub const PREFER_MODEL_CONFIG_QUERY: &str =
    include_str!("queries/backends/prefer_model_config.sql");

pub const SET_PREFERRED_QUERY: &str = include_str!("queries/backends/set_preferred.sql");

pub const MATCH_PERMISSION_QUERY: &str = include_str!("queries/permissions/match_permission.sql");

pub const INSERT_PERMISSION_QUERY: &str = include_str!("queries/permissions/insert_permission.sql");

pub const GET_PERMISSIONS_QUERY: &str = include_str!("queries/permissions/get_permissions.sql");

pub const DELETE_PERMISSION_QUERY: &str = include_str!("queries/permissions/delete_permission.sql");

pub const INSERT_HISTORY: &str = include_str!("queries/history/insert_history.sql");

pub const GET_HISTORY: &str = include_str!("queries/history/get_history.sql");

pub const RESTORE_HISTORY: &str = include_str!("queries/history/restore_history.sql");

pub const RECOVER: &str = include_str!("queries/history/recover_history.sql");

pub const ROLLBACK: &str = include_str!("queries/history/rollback_history.sql");

pub const DELETE_EMPTY_SESSION: &str = include_str!("queries/sessions/delete_empty_session.sql");

pub const DELETE_ALL_EMPTY_SESSIONS: &str =
    include_str!("queries/sessions/delete_all_empty_sessions.sql");

pub const INSERT_PLUGIN_QUERY: &str = include_str!("queries/plugins/insert_plugin.sql");

pub const GET_PLUGINS_BY_TYPE_QUERY: &str = include_str!("queries/plugins/get_plugins_by_type.sql");

pub const GET_PLUGIN_CREDENTIAL_QUERY: &str =
    include_str!("queries/plugins/get_plugin_credential.sql");

pub const UPDATE_PLUGIN_CREDENTIAL_QUERY: &str =
    include_str!("queries/plugins/update_credential.sql");

pub const DISABLED_PLUGINS_QUERY: &str = include_str!("queries/plugins/disabled_plugins.sql");

pub const DELETE_PLUGIN_QUERY: &str = include_str!("queries/plugins/delete_plugin.sql");

pub const DISABLE_PLUGIN_QUERY: &str = include_str!("queries/plugins/toggle_plugin.sql");

pub const UPDATE_PLUGIN_QUERY: &str = include_str!("queries/plugins/update_plugin.sql");
