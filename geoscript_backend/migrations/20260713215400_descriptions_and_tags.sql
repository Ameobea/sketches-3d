ALTER TABLE textures ADD COLUMN description TEXT NOT NULL DEFAULT '' CHECK (LENGTH(description) <= 2000);
ALTER TABLE materials ADD COLUMN description TEXT NOT NULL DEFAULT '' CHECK (LENGTH(description) <= 2000);

CREATE TABLE tags (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  tag TEXT NOT NULL UNIQUE,
  CHECK (LENGTH(tag) > 0 AND LENGTH(tag) <= 64)
);

CREATE TABLE texture_tags (
  texture_id INTEGER NOT NULL REFERENCES textures(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (texture_id, tag_id)
) WITHOUT ROWID;

CREATE TABLE material_tags (
  material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (material_id, tag_id)
) WITHOUT ROWID;

CREATE TABLE composition_tags (
  composition_id INTEGER NOT NULL REFERENCES compositions(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (composition_id, tag_id)
) WITHOUT ROWID;

CREATE INDEX idx_texture_tags_tag_id ON texture_tags(tag_id);
CREATE INDEX idx_material_tags_tag_id ON material_tags(tag_id);
CREATE INDEX idx_composition_tags_tag_id ON composition_tags(tag_id);
