-- Tree v1: stamp a short (8 hex char) `id` on every node instance that lacks one.
-- An instance id addresses one placement for gizmo targeting and undo; uniqueness only
-- needs to hold within a node's `instances` array (refs always carry the node id), so a
-- random 32-bit value makes within-node collisions vanishingly unlikely. Existing ids are
-- preserved, so re-application is a no-op.

UPDATE composition_versions
SET tree = json_set(tree, '$.nodes', (
  SELECT json_group_object(
    n.key,
    json_set(n.value, '$.instances', (
      SELECT json_group_array(
        CASE
          WHEN i.value ->> '$.id' IS NULL
            THEN json_set(i.value, '$.id', lower(hex(randomblob(4))))
          ELSE i.value
        END
      )
      FROM json_each(n.value, '$.instances') i
    ))
  )
  FROM json_each(composition_versions.tree, '$.nodes') n
))
WHERE tree ->> '$.version' = 1;
