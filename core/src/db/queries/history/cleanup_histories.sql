-- cache all sessions + each the latest prompt id and the latest non-reasoning row
CREATE TEMP TABLE cleanup_turns AS
SELECT h.session_id,
       t.start,
       MAX(h.id) AS last_item
FROM history h
         JOIN (SELECT session_id, MAX(id) AS start
               FROM history
               WHERE payload_type = 'user_prompt'
               GROUP BY session_id) t ON t.session_id = h.session_id
WHERE h.id >= t.start
  AND h.payload_type <> 'reasoning'
GROUP BY h.session_id;

-- improve performance, every step below looks turns up by session id
CREATE INDEX cleanup_turns_session ON cleanup_turns (session_id);

-- Step A: delete all trailing reasoning rows, OpenAI rejects a reasoning item without following assistant or tool items.
DELETE
FROM history
WHERE payload_type = 'reasoning'
  AND id > (SELECT t.last_item
            FROM cleanup_turns t
            WHERE t.session_id = history.session_id);

-- Step B: if the turn is only the user prompt, we clean up the whole turn.
DELETE
FROM history
WHERE id >= (SELECT t.start
             FROM cleanup_turns t
             WHERE t.session_id = history.session_id
               AND t.last_item = t.start);

-- Step C: if current turn has more such as tool calls, we should add cancellation message as tool call result.
INSERT INTO history (session_id, provider_id, backend_id, payload_type, payload)
SELECT h.session_id,
       h.provider_id,
       h.backend_id,
       'tool_result',
       json_object('kind', 'tool_result',
                   'call_id', h.payload ->> '$.call_id',
                   'name', h.payload ->> '$.name',
                   'output', ?1)
FROM history h
         JOIN cleanup_turns t ON t.session_id = h.session_id
WHERE h.payload_type = 'tool_call'
  AND h.id >= t.start
  AND NOT EXISTS (SELECT 1
                  FROM history o
                  WHERE o.session_id = h.session_id
                    AND o.payload_type = 'tool_result'
                    AND o.payload ->> '$.call_id' = h.payload ->> '$.call_id');

-- Step D: delete all sessions with no history left
DELETE
FROM sessions
WHERE NOT EXISTS (SELECT 1
                  FROM history
                  WHERE history.session_id = sessions.session_id);

-- cleanup
DROP TABLE cleanup_turns;
