SELECT payload_type, payload
FROM history
WHERE session_id = ?
ORDER BY id;
