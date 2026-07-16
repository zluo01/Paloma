SELECT preferred_provider_id AS provider_id,
       preferred_backend_id  AS backend_id
FROM settings
WHERE id = 0
  AND preferred_provider_id IS NOT NULL
  AND preferred_backend_id IS NOT NULL;
