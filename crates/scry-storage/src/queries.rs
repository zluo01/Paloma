//language=sqlite
pub const INIT_TABLE_QUERY: &str = "
  CREATE TABLE IF NOT EXISTS provider_credentials (
    provider_id TEXT PRIMARY KEY NOT NULL,
    auth_kind TEXT NOT NULL CHECK (auth_kind IN ('api_key', 'oauth')),
    secret TEXT NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL
  );
    ";

//language=sqlite
pub const INSERT_PROVIDER_QUERY: &str = "
  INSERT INTO provider_credentials (provider_id, auth_kind, secret, model, effort)
  VALUES (?, ?, ?, ?, ?);
    ";

//language=sqlite
pub const UPDATE_PROVIDER_QUERY: &str = "
  UPDATE provider_credentials
  SET auth_kind = ?, secret = ?
  WHERE provider_id = ?;
    ";

//language=sqlite
pub const UPDATE_PROVIDER_PREFERENCES_QUERY: &str = "
  UPDATE provider_credentials
  SET model = ?, effort = ?
  WHERE provider_id = ?;
    ";

//language=sqlite
pub const DELETE_PROVIDER_QUERY: &str = "
  DELETE FROM provider_credentials
  WHERE provider_id = ?;
    ";

//language=sqlite
pub const CONNECTED_PROVIDERS_QUERY: &str = "
  SELECT * FROM provider_credentials
    ";
