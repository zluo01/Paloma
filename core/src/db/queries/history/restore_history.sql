SELECT h.provider_id,
       h.payload,
       EXISTS (SELECT 1
               FROM history o
               WHERE o.session_id = h.session_id
                 AND o.payload_type = 'tool_result'
                 AND o.payload ->> '$.call_id' = h.payload ->> '$.call_id') AS finished
FROM history h
WHERE h.session_id = ?
  AND h.payload_type <> 'tool_result'
ORDER BY h.id;
