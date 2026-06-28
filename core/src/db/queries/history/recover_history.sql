DELETE
FROM history
WHERE session_id IN (SELECT last.session_id
                     FROM history last
                     WHERE last.id = (SELECT MAX(h.id)
                                      FROM history h
                                      WHERE h.session_id = last.session_id)
                       AND last.payload_type <> 'message')
  AND id >= (SELECT MAX(prompt.id)
             FROM history prompt
             WHERE prompt.session_id = history.session_id
               AND prompt.payload_type = 'user_prompt');
