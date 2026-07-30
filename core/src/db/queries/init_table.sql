CREATE TABLE IF NOT EXISTS backend_credentials
(
    provider_id TEXT NOT NULL,
    backend_id  TEXT NOT NULL,
    auth_kind   TEXT NOT NULL CHECK (auth_kind IN ('api_key', 'oauth')),
    secret      TEXT NOT NULL,
    model       TEXT NOT NULL,
    effort      TEXT NOT NULL,
    PRIMARY KEY (provider_id, backend_id)
);

CREATE TABLE IF NOT EXISTS settings
(
    id                    INTEGER PRIMARY KEY CHECK (id = 0),
    preferred_provider_id TEXT,
    preferred_backend_id  TEXT,
    FOREIGN KEY (preferred_provider_id, preferred_backend_id)
        REFERENCES backend_credentials (provider_id, backend_id)
        ON DELETE SET NULL
);

INSERT OR IGNORE INTO settings (id)
VALUES (0);

-- set preferred on first insert when no preferred is set
CREATE TRIGGER IF NOT EXISTS settings_prefer_first_backend
    AFTER INSERT
    ON backend_credentials
    WHEN (SELECT preferred_provider_id
          FROM settings) IS NULL
BEGIN
    UPDATE settings
    SET preferred_provider_id = NEW.provider_id,
        preferred_backend_id  = NEW.backend_id
    WHERE id = 0;
END;

-- auto set preferred on preferred deletion
CREATE TRIGGER IF NOT EXISTS settings_reassign_preferred_on_delete
    BEFORE DELETE
    ON backend_credentials
    WHEN OLD.provider_id = (SELECT preferred_provider_id
                            FROM settings)
        AND OLD.backend_id = (SELECT preferred_backend_id
                              FROM settings)
BEGIN
    UPDATE settings
    SET (preferred_provider_id, preferred_backend_id) =
            (SELECT provider_id, backend_id
             FROM backend_credentials
             WHERE NOT (provider_id = OLD.provider_id AND backend_id = OLD.backend_id)
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
    backend_id   TEXT    NOT NULL,
    payload_type TEXT    NOT NULL,
    payload      TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_session ON history (session_id);

CREATE TRIGGER IF NOT EXISTS sessions_touch_on_user_prompt
    AFTER INSERT
    ON history
    WHEN NEW.payload_type = 'user_prompt'
BEGIN
    UPDATE sessions
    SET last_update = NEW.timestamp
    WHERE session_id = NEW.session_id;
END;

CREATE TABLE IF NOT EXISTS plugins
(
    name        TEXT PRIMARY KEY NOT NULL,
    plugin_type TEXT             NOT NULL CHECK (plugin_type IN ('extension', 'provider', 'mcp')),
    transport   TEXT             NOT NULL CHECK (transport IN ('local', 'http')),
    timeout     INTEGER          NOT NULL DEFAULT 300 CHECK (timeout > 0),
    disabled    INTEGER          NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    env         TEXT             NOT NULL DEFAULT '{}',
    args        TEXT             NOT NULL,
    credential  TEXT,
    creation    INTEGER          NOT NULL DEFAULT (unixepoch()),
    -- Extension and provider plugins are spawned as local subprocesses,
    -- only mcp plugins may go over http.
    CHECK (plugin_type = 'mcp' OR transport = 'local')
);

-- deleting a provider plugin removes every credential stored for its backends
CREATE TRIGGER IF NOT EXISTS plugins_delete_backend_credentials
    AFTER DELETE
    ON plugins
    WHEN OLD.plugin_type = 'provider'
BEGIN
    DELETE
    FROM backend_credentials
    WHERE provider_id = OLD.name;
END;

CREATE TABLE IF NOT EXISTS disabled_capabilities
(
    plugin_name   TEXT NOT NULL, -- extension id or mcp name
    capability_id TEXT NOT NULL, -- capability id or mcp tool name
    facet         TEXT NOT NULL CHECK (facet IN ('search', 'tool', 'mcp')),
    PRIMARY KEY (plugin_name, capability_id, facet)
);

-- deleting a plugin removes its per-capability disable flags
CREATE TRIGGER IF NOT EXISTS plugins_delete_disabled_capabilities
    AFTER DELETE
    ON plugins
BEGIN
    DELETE
    FROM disabled_capabilities
    WHERE plugin_name = OLD.name;
END;
