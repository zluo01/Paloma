SELECT name, transport, timeout, disabled, env, args
FROM plugins
WHERE plugin_type = ?
ORDER BY creation DESC;
