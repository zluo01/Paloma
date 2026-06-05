UPDATE provider_credentials
SET auth_kind = ?,
    secret    = ?
WHERE provider_id = ?;
