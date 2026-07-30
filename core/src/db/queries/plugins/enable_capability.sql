DELETE
FROM disabled_capabilities
WHERE plugin_name = ?
  AND capability_id = ?
  AND facet = ?;
