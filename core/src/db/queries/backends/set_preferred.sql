UPDATE settings
SET preferred_provider_id = ?,
    preferred_backend_id  = ?
WHERE id = 0;
