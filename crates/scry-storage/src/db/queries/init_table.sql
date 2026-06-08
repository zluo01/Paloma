CREATE TABLE IF NOT EXISTS provider_credentials
(
    provider_id TEXT PRIMARY KEY NOT NULL,
    auth_kind   TEXT             NOT NULL CHECK (auth_kind IN ('api_key', 'oauth')),
    secret      TEXT             NOT NULL,
    model       TEXT             NOT NULL,
    effort      TEXT             NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions
(
    session_id  TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT
                                 REFERENCES provider_credentials (provider_id)
                                     ON DELETE SET NULL,
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
    id          INTEGER PRIMARY KEY,
    session_id  TEXT    NOT NULL
        REFERENCES sessions (session_id)
            ON DELETE CASCADE,
    timestamp   INTEGER NOT NULL DEFAULT (unixepoch()),
    payload_type TEXT    NOT NULL,
    payload     TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_session ON history (session_id);
