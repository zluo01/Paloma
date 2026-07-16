SELECT bc.*,
       EXISTS (SELECT 1
               FROM settings
               WHERE preferred_provider_id = bc.provider_id
                 AND preferred_backend_id = bc.backend_id) AS preferred
FROM backend_credentials bc;
