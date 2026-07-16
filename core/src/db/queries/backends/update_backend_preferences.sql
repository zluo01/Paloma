UPDATE backend_credentials
SET model  = ?,
    effort = ?
WHERE provider_id = ?
  AND backend_id = ?;
