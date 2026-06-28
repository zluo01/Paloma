DELETE
FROM history
WHERE session_id = ?
  AND id >= (SELECT MAX(prompt.id)
             FROM history prompt
             WHERE prompt.session_id = history.session_id
               AND prompt.payload_type = 'user_prompt');
