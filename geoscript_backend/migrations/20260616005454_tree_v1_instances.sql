-- Tree schema v0 -> v1: stamp `version: 1` and replace each node's single `transform`
-- with `instances: [transform]` (a one-element list). v1 expresses per-node placement as a
-- list of transforms; the single-copy case is `instances.length === 1`, identical to v0.
-- Guarded on the absent `version` field so re-application is a no-op.

UPDATE composition_versions
SET tree = json_object(
  'version', 1,
  'rootId', tree ->> '$.rootId',
  'globalsSource', tree ->> '$.globalsSource',
  'nodes', (
    SELECT json_group_object(
      n.key,
      json_set(
        json_remove(n.value, '$.transform'),
        '$.instances',
        json_array(json(n.value ->> '$.transform'))
      )
    )
    FROM json_each(composition_versions.tree, '$.nodes') n
  )
)
WHERE tree ->> '$.version' IS NULL;
