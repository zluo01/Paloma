SELECT provider_id, backend_id, payload
FROM history
WHERE session_id = ?
ORDER BY id;
