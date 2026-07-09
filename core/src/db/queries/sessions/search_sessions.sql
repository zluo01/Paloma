SELECT DISTINCT h.session_id
FROM history h
WHERE (h.payload_type = 'user_prompt'
    AND h.payload ->> '$.prompt' LIKE ? ESCAPE '\')
   OR (h.payload_type = 'message'
    AND EXISTS (SELECT 1
                FROM json_each(h.payload, '$.message') item
                WHERE item.value ->> '$.content' LIKE ? ESCAPE '\'));
