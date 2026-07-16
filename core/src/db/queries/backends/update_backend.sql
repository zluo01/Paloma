UPDATE backend_credentials
SET auth_kind = ?,
    secret    = ?
WHERE provider_id = ?
  AND backend_id = ?;
