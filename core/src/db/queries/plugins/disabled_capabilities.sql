SELECT plugin_name, capability_id, facet
FROM disabled_capabilities
WHERE facet IN (SELECT value FROM json_each(?));
