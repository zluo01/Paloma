INSERT INTO permissions (prefix, with_glob)
VALUES (?, ?)
ON CONFLICT(prefix) DO UPDATE SET with_glob  = excluded.with_glob,
                                  updated_at = unixepoch();
