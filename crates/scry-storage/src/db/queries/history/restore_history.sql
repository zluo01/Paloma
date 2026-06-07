SELECT h.payloadType,
       h.payload,
       EXISTS (SELECT 1
               FROM history o
               WHERE o.session_id = h.session_id
                 AND o.payload ->> '$.type' = 'function_call_output'
                 AND o.payload ->> '$.call_id' = h.payload ->> '$.call_id') AS finished
FROM history h
WHERE h.session_id = ?
  AND h.payload ->> '$.type' IS NOT 'function_call_output'
ORDER BY h.id;
