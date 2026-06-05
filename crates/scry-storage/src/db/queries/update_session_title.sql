UPDATE sessions
SET title     = ?,
    generated = 1
WHERE session_id = ?;
