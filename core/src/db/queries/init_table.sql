CREATE TABLE IF NOT EXISTS provider_credentials
(
    provider_id TEXT PRIMARY KEY NOT NULL,
    auth_kind   TEXT             NOT NULL CHECK (auth_kind IN ('api_key', 'oauth')),
    secret      TEXT             NOT NULL,
    model       TEXT             NOT NULL,
    effort      TEXT             NOT NULL
);

CREATE TABLE IF NOT EXISTS settings
(
    id                 INTEGER PRIMARY KEY CHECK (id = 0),
    preferred_provider TEXT
        REFERENCES provider_credentials (provider_id)
            ON DELETE SET NULL
);

INSERT OR IGNORE INTO settings (id)
VALUES (0);

-- set preferred on first insert when no preferred is set
CREATE TRIGGER IF NOT EXISTS settings_prefer_first_provider
    AFTER INSERT
    ON provider_credentials
    WHEN (SELECT preferred_provider
          FROM settings) IS NULL
BEGIN
    UPDATE settings SET preferred_provider = NEW.provider_id WHERE id = 0;
END;

-- auto set preferred on preferred deletion
CREATE TRIGGER IF NOT EXISTS settings_reassign_preferred_on_delete
    BEFORE DELETE
    ON provider_credentials
    WHEN OLD.provider_id = (SELECT preferred_provider
                            FROM settings)
BEGIN
    UPDATE settings
    SET preferred_provider = (SELECT provider_id
                              FROM provider_credentials
                              WHERE provider_id <> OLD.provider_id
                              ORDER BY rowid
                              LIMIT 1)
    WHERE id = 0;
END;

CREATE TABLE IF NOT EXISTS sessions
(
    session_id  TEXT PRIMARY KEY NOT NULL,
    title       TEXT             NOT NULL DEFAULT '',
    last_update INTEGER          NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS permissions
(
    prefix     TEXT PRIMARY KEY NOT NULL CHECK (length(prefix) > 0),
    with_glob  INTEGER          NOT NULL DEFAULT 0 CHECK (with_glob IN (0, 1)),
    updated_at INTEGER          NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS history
(
    id           INTEGER PRIMARY KEY,
    session_id   TEXT    NOT NULL
        REFERENCES sessions (session_id)
            ON DELETE CASCADE,
    timestamp    INTEGER NOT NULL DEFAULT (unixepoch()),
    provider_id  TEXT    NOT NULL,
    payload_type TEXT    NOT NULL,
    payload      TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_session ON history (session_id);

CREATE TABLE IF NOT EXISTS plugins
(
    name        TEXT PRIMARY KEY NOT NULL,
    plugin_type TEXT             NOT NULL CHECK (plugin_type IN ('native', 'mcp')),
    transport   TEXT             NOT NULL CHECK (transport IN ('local', 'http')),
    timeout     INTEGER          NOT NULL DEFAULT 300 CHECK (timeout > 0),
    disabled    INTEGER          NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    env         TEXT             NOT NULL DEFAULT '{}',
    args        TEXT             NOT NULL,
    creation    INTEGER          NOT NULL DEFAULT (unixepoch()),
    -- Native plugins run in-process; only mcp plugins may go over http.
    CHECK (NOT (plugin_type = 'native' AND transport = 'http'))
);
