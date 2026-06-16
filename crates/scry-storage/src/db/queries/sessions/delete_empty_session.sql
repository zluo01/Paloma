DELETE
FROM sessions
WHERE session_id = ?
  AND NOT EXISTS (SELECT 1
                  FROM history
                  WHERE history.session_id = sessions.session_id
                    AND history.payload ->> '$.type' = 'message'
                    AND history.payload ->> '$.status' = 'completed');
