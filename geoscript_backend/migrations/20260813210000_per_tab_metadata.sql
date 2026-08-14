-- Version metadata v1: per-tab state moves out of the composition-wide bag into a `tabs`
-- record keyed by tree id, tagged by the tree's kind.
--
--   {views:{id:view}, activeTreeId, materials, preludeEjected, environment}
--     -> {version:1, tabs:{id:{kind, preludeEjected, view?, environment?}}, activeTreeId, materials?}
--
-- `view` and `environment` are mesh-only and so land only on mesh entries. Tab ids come from
-- the tree column, so `views` entries for trees that no longer exist are dropped rather than
-- carried forward. Runs after the v2 container migration, which guarantees `$.trees`.
--
-- The `json_object('view', <NULL>)` in the patch is deliberate: RFC-7396 merge semantics
-- delete a key whose patch value is null, which is exactly the "no saved view" case.

UPDATE composition_versions
SET metadata = json_patch(
  json_object(
    'version', 1,
    'activeTreeId', coalesce(metadata ->> '$.activeTreeId', tree ->> '$.trees[0].id'),
    'tabs', (
      SELECT json_group_object(
        t.value ->> '$.id',
        CASE t.value ->> '$.kind'
          WHEN 'mesh' THEN json_patch(
            json_object(
              'kind', 'mesh',
              'preludeEjected', json(CASE WHEN metadata ->> '$.preludeEjected' THEN 'true' ELSE 'false' END)
            ),
            json_object(
              'view', (
                SELECT json_set(
                  json_extract(metadata, '$.views."' || (t.value ->> '$.id') || '"'),
                  '$.projection',
                  coalesce(
                    json_extract(metadata, '$.views."' || (t.value ->> '$.id') || '".projection'),
                    'perspective'
                  )
                )
              ),
              'environment', json_extract(metadata, '$.environment')
            )
          )
          ELSE json_object(
            'kind', 'texture',
            'preludeEjected', json(CASE WHEN metadata ->> '$.preludeEjected' THEN 'true' ELSE 'false' END)
          )
        END
      )
      FROM json_each(composition_versions.tree, '$.trees') AS t
    )
  ),
  json_object('materials', json_extract(metadata, '$.materials'))
)
WHERE json_extract(metadata, '$.version') IS NULL;
