DELETE
FROM sessions
WHERE NOT EXISTS (SELECT 1
                  FROM history
                  WHERE history.session_id = sessions.session_id);
