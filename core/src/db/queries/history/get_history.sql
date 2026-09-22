SELECT h.id,
       h.provider_id,
       h.backend_id,
       h.payload,
       a.ordinal,
       a.kind,
       a.media_type,
       a.data
FROM history h
         LEFT JOIN attachments a ON a.history_id = h.id
WHERE h.session_id = ?
ORDER BY h.id, a.ordinal;
