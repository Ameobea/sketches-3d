-- Convert any already-forest-shaped TreeDef rows (`rootIds: [<id>]`) to the singular
-- `rootId` shape with the root node renamed to `_root`. New databases produced by the
-- earlier migration already land in this shape directly, so this migration is a
-- no-op for them. It exists to bring already-migrated DBs (which output the
-- transitional forest shape) into the final shape without a manual reset.
--
-- Assumes single-rootId (no multi-root composition was ever created in prod).

UPDATE composition_versions
SET tree = (
  WITH this AS (
    SELECT
      json_extract(tree, '$.rootIds[0]') AS root_id,
      COALESCE(json_extract(tree, '$.globalsSource'), '') AS globals_source,
      tree AS old_tree
  ),
  renamed_nodes AS (
    SELECT json_group_object(
      nodes.key,
      CASE
        WHEN nodes.key = this.root_id
        THEN json_set(nodes.value, '$.name', '_root')
        ELSE nodes.value
      END
    ) AS nodes_json
    FROM this, json_each(this.old_tree, '$.nodes') AS nodes
  )
  SELECT json_object(
    'rootId', this.root_id,
    'globalsSource', this.globals_source,
    'nodes', json(renamed_nodes.nodes_json)
  )
  FROM this, renamed_nodes
)
WHERE json_extract(tree, '$.rootIds') IS NOT NULL;
