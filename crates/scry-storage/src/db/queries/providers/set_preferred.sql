UPDATE provider_credentials
SET preferred = (provider_id = ?)
WHERE EXISTS (SELECT 1 FROM provider_credentials WHERE provider_id = ?);
