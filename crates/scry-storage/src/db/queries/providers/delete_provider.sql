-- Reset sqlite3_changes() before transaction statements so NotFound stays reliable.
UPDATE provider_credentials
SET preferred = preferred
WHERE 0;

BEGIN IMMEDIATE;

UPDATE provider_credentials
SET preferred = 1
WHERE provider_id = (
    SELECT provider_id
    FROM provider_credentials
    WHERE provider_id <> ?1
    ORDER BY rowid
    LIMIT 1
)
  AND EXISTS (
    SELECT 1
    FROM provider_credentials
    WHERE provider_id = ?1
      AND preferred = 1
);

DELETE
FROM provider_credentials
WHERE provider_id = ?1;

COMMIT;
