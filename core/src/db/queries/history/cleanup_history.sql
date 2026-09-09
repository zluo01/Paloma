-- get and cache the latest prompt id
CREATE TEMP TABLE cleanup_turn AS
SELECT MAX(id) AS start
FROM history
WHERE session_id = ?1
  AND payload_type = 'user_prompt';

-- Step A: delete all trailing reasoning row, OpenAI rejects a reasoning item without following assistant or tool items.
DELETE
FROM history
WHERE session_id = ?1
  AND payload_type = 'reasoning'
  AND id > (SELECT MAX(id)
            FROM history
            WHERE session_id = ?1
              AND id >= (SELECT start FROM cleanup_turn)
              AND payload_type <> 'reasoning');

-- Step B: if the turn is only the user prompt, we clean up the whole turn
DELETE
FROM history
WHERE session_id = ?1
  AND id >= (SELECT start FROM cleanup_turn)
  AND (SELECT COUNT(*)
       FROM history
       WHERE session_id = ?1
         AND id >= (SELECT start FROM cleanup_turn)) = 1;

-- Step C: if current turn has more such as tool calls, we should add cancellation message as tool call result.
INSERT INTO history (session_id, provider_id, backend_id, payload_type, payload)
SELECT h.session_id,
       h.provider_id,
       h.backend_id,
       'tool_result',
       json_object('kind', 'tool_result',
                   'call_id', h.payload ->> '$.call_id',
                   'name', h.payload ->> '$.name',
                   'output', ?2)
FROM history h
WHERE h.session_id = ?1
  AND h.payload_type = 'tool_call'
  AND h.id >= (SELECT start FROM cleanup_turn)
  AND NOT EXISTS (SELECT 1
                  FROM history o
                  WHERE o.session_id = ?1
                    AND o.payload_type = 'tool_result'
                    AND o.payload ->> '$.call_id' = h.payload ->> '$.call_id');

-- Step D: delete the session if there is no history exists(Step B is true, and it is first prompt of the session)
-- returns the session id when the session was removed, nothing otherwise
DELETE
FROM sessions
WHERE session_id = ?1
  AND NOT EXISTS (SELECT 1
                  FROM history
                  WHERE history.session_id = sessions.session_id)
RETURNING session_id;

-- cleanup
DROP TABLE cleanup_turn;
