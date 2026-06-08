SELECT name, transport, timeout, disabled, env, args
FROM plugins
WHERE plugin_type = 'mcp'
ORDER BY name;
