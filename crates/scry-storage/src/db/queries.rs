pub const INIT_TABLE_QUERY: &str = include_str!("queries/init_table.sql");

pub const INSERT_PROVIDER_QUERY: &str = include_str!("queries/insert_provider.sql");

pub const UPDATE_PROVIDER_QUERY: &str = include_str!("queries/update_provider.sql");

pub const UPDATE_PROVIDER_PREFERENCES_QUERY: &str =
    include_str!("queries/update_provider_preferences.sql");

pub const DELETE_PROVIDER_QUERY: &str = include_str!("queries/delete_provider.sql");

pub const CONNECTED_PROVIDERS_QUERY: &str = include_str!("queries/connected_providers.sql");

pub const CREATE_NEW_SESSION_QUERY: &str = include_str!("queries/create_new_session.sql");

pub const UPDATE_SESSION_TITLE_QUERY: &str = include_str!("queries/update_session_title.sql");

pub const TOUCH_SESSION_QUERY: &str = include_str!("queries/touch_session.sql");

pub const GET_ALL_SESSIONS_QUERY: &str = include_str!("queries/get_all_sessions.sql");

pub const DELETE_SESSION_QUERY: &str = include_str!("queries/delete_session.sql");

pub const PREFER_MODEL_CONFIG_QUERY: &str = include_str!("queries/prefer_model_config.sql");
