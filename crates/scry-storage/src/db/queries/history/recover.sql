-- For every session whose newest history item isn't a completed assistant
-- message, delete from the last user prompt (inclusive) to the end: that turn
-- never finished, so its items are invalid input for the next request.
DELETE
FROM history
WHERE session_id IN (SELECT last.session_id
                     FROM history last
                     WHERE last.id = (SELECT MAX(h.id)
                                      FROM history h
                                      WHERE h.session_id = last.session_id)
                       AND (COALESCE(last.payload ->> '$.type', '') <> 'message'
                         OR COALESCE(last.payload ->> '$.status', '') <> 'completed'))
  AND id >= (SELECT MAX(prompt.id)
             FROM history prompt
             WHERE prompt.session_id = history.session_id
               AND prompt.payload ->> '$.type' = 'message'
               AND prompt.payload ->> '$.role' = 'user');
