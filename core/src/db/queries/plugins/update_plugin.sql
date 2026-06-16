UPDATE plugins
SET transport = ?,
    timeout   = ?,
    env       = ?,
    args      = ?
WHERE name = ?;
