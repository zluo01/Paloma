SELECT h.provider_id,
       h.backend_id,
       h.payload,
       COALESCE(h.payload ->> '$.call_id' IN (SELECT o.payload ->> '$.call_id'
                                              FROM history o
                                              WHERE o.session_id = ?1
                                                AND o.payload_type = 'tool_result'), 0) AS finished
FROM history h
WHERE h.session_id = ?1
  AND h.payload_type <> 'tool_result'
ORDER BY h.id;
