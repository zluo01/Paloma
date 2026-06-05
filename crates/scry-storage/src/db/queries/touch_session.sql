UPDATE sessions
SET last_update = unixepoch()
WHERE session_id = ?;
