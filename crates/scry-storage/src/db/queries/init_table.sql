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
    generated   INTEGER          NOT NULL DEFAULT 0 CHECK (generated IN (0, 1)),
    last_update INTEGER          NOT NULL DEFAULT (unixepoch())
);
