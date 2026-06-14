pub const INIT_TABLE_QUERY: &str = include_str!("queries/init_table.sql");

pub const INSERT_PROVIDER_QUERY: &str = include_str!("queries/providers/insert_provider.sql");

pub const UPDATE_PROVIDER_QUERY: &str = include_str!("queries/providers/update_provider.sql");

pub const UPDATE_PROVIDER_PREFERENCES_QUERY: &str =
    include_str!("queries/providers/update_provider_preferences.sql");

pub const DELETE_PROVIDER_QUERY: &str = include_str!("queries/providers/delete_provider.sql");

pub const CONNECTED_PROVIDERS_QUERY: &str =
    include_str!("queries/providers/connected_providers.sql");

pub const CREATE_NEW_SESSION_QUERY: &str = include_str!("queries/sessions/create_new_session.sql");

pub const TOUCH_SESSION_QUERY: &str = include_str!("queries/sessions/touch_session.sql");

pub const GET_ALL_SESSIONS_QUERY: &str = include_str!("queries/sessions/get_all_sessions.sql");

pub const DELETE_SESSION_QUERY: &str = include_str!("queries/sessions/delete_session.sql");

pub const PREFER_MODEL_CONFIG_QUERY: &str =
    include_str!("queries/providers/prefer_model_config.sql");

pub const SET_PREFERRED_QUERY: &str = include_str!("queries/providers/set_preferred.sql");

pub const MATCH_PERMISSION_QUERY: &str = include_str!("queries/permissions/match_permission.sql");

pub const INSERT_PERMISSION_QUERY: &str = include_str!("queries/permissions/insert_permission.sql");

pub const INSERT_HISTORY: &str = include_str!("queries/history/insert_history.sql");

pub const GET_HISTORY: &str = include_str!("queries/history/get_history.sql");

pub const RESTORE_HISTORY: &str = include_str!("queries/history/restore_history.sql");

pub const INSERT_PLUGIN_QUERY: &str = include_str!("queries/plugins/insert_plugin.sql");

pub const GET_ALL_MCP_QUERY: &str = include_str!("queries/plugins/get_all_mcp.sql");

pub const DISABLED_PLUGINS_QUERY: &str = include_str!("queries/plugins/disabled_plugins.sql");

pub const DELETE_PLUGIN_QUERY: &str = include_str!("queries/plugins/delete_plugin.sql");

pub const DISABLE_PLUGIN_QUERY: &str = include_str!("queries/plugins/toggle_plugin.sql");

pub const UPDATE_PLUGIN_QUERY: &str = include_str!("queries/plugins/update_plugin.sql");
