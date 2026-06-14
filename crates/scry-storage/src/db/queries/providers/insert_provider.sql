INSERT INTO provider_credentials (provider_id, auth_kind, secret, model, effort, preferred)
VALUES (?, ?, ?, ?, ?, NOT EXISTS (SELECT 1 FROM provider_credentials));
