SELECT pc.*,
       (pc.provider_id IS (SELECT preferred_provider FROM settings)) AS preferred
FROM provider_credentials pc;
