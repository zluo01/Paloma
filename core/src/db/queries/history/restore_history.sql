SELECT h.id,
       h.provider_id,
       h.backend_id,
       h.payload,
       COALESCE(h.payload ->> '$.call_id' IN (SELECT o.payload ->> '$.call_id'
                                              FROM history o
                                              WHERE o.session_id = ?1
                                                AND o.payload_type = 'tool_result'), 0) AS finished,
       a.ordinal,
       a.kind,
       a.media_type,
       a.data
FROM history h
         LEFT JOIN attachments a ON a.history_id = h.id
WHERE h.session_id = ?1
  AND h.payload_type <> 'tool_result'
ORDER BY h.id, a.ordinal;
