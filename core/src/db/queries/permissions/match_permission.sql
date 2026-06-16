SELECT EXISTS (SELECT 1
               FROM permissions
               WHERE prefix = ?
                  OR (with_glob = 1 AND substr(?, 1, length(prefix) + 1) = prefix || ' '));
