SELECT model, effort
FROM backend_credentials
WHERE provider_id = ?
  AND backend_id = ?;
