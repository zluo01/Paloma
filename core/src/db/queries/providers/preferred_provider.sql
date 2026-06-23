SELECT preferred_provider
FROM settings
WHERE id = 0
  AND preferred_provider IS NOT NULL;
