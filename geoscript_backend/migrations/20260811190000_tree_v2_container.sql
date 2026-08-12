-- Tree schema v1 -> v2: wrap each version's single tree in a container of typed trees
-- ({version: 2, trees: [{id, kind, name, tree: <intact v1 core>}]}), the sole existing
-- tree becoming the 'main' mesh tree. The singular metadata `view` moves to per-tree
-- `views` keyed by tree id, with `activeTreeId` selecting it. Both statements are
-- guarded so re-application is a no-op.

UPDATE composition_versions
SET tree = json_object(
  'version', 2,
  'trees', json_array(json_object(
    'id', 'main',
    'kind', 'mesh',
    'name', 'main',
    'tree', json(tree)
  ))
)
WHERE tree ->> '$.version' = 1;

UPDATE composition_versions
SET metadata = json_remove(
  json_set(metadata,
    '$.views', json_object('main', metadata -> '$.view'),
    '$.activeTreeId', 'main'),
  '$.view')
WHERE json_extract(metadata, '$.view') IS NOT NULL;
