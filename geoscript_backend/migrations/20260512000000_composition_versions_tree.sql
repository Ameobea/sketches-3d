-- Replace flat `source_code` on composition_versions with a tree-shaped JSON document.
-- Each existing row is wrapped as a single-root TreeDef whose only node carries the previous
-- source code. After this migration, the application still renders single-buffer compositions
-- identically; the data shape is now ready for hierarchical composition.

CREATE TABLE composition_versions_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  composition_id INTEGER NOT NULL,
  tree JSON NOT NULL,
  metadata JSON NOT NULL DEFAULT '{}',
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  thumbnail_url TEXT,
  FOREIGN KEY (composition_id) REFERENCES compositions(id) ON DELETE CASCADE,
  CHECK (LENGTH(tree) <= 1000000),
  CHECK (LENGTH(metadata) <= 500000)
);

WITH cv AS MATERIALIZED (
  SELECT
    id,
    composition_id,
    source_code,
    metadata,
    created_at,
    thumbnail_url,
    lower(hex(randomblob(16))) AS node_id
  FROM composition_versions
)
INSERT INTO composition_versions_new (id, composition_id, tree, metadata, created_at, thumbnail_url)
SELECT
  cv.id,
  cv.composition_id,
  json_object(
    'rootIds', json_array(cv.node_id),
    'globalsSource', '',
    'nodes', json_object(
      cv.node_id,
      json_object(
        'id', cv.node_id,
        'name', 'main',
        'source', cv.source_code,
        'transform', json_object(
          'pos', json_array(0, 0, 0),
          'rot', json_array(0, 0, 0),
          'scale', json_array(1, 1, 1)
        ),
        'children', json_array()
      )
    )
  ),
  cv.metadata,
  cv.created_at,
  cv.thumbnail_url
FROM cv;

DROP TABLE composition_versions;
ALTER TABLE composition_versions_new RENAME TO composition_versions;

CREATE INDEX idx_composition_versions_composition_id ON composition_versions(composition_id);

DROP TRIGGER IF EXISTS delete_thumbnails;
CREATE TRIGGER delete_thumbnails
AFTER DELETE ON composition_versions
FOR EACH ROW
WHEN OLD.thumbnail_url IS NOT NULL
BEGIN
  INSERT INTO deleted_thumbnails (url) VALUES (OLD.thumbnail_url);
END;
