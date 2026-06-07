SELECT payloadType, payload
FROM history
WHERE session_id = ?
ORDER BY id;
