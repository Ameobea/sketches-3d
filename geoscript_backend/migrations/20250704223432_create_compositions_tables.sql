CREATE TABLE compositions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  author_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  forked_from_id INTEGER,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (forked_from_id) REFERENCES compositions(id) ON DELETE SET NULL,
  CHECK (LENGTH(title) <= 1000),
  CHECK (LENGTH(description) <= 100000)
);

CREATE INDEX idx_compositions_author_id ON compositions(author_id);

CREATE TRIGGER update_composition_updated_at
AFTER UPDATE ON compositions
FOR EACH ROW
BEGIN
  UPDATE compositions SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TABLE composition_versions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  composition_id INTEGER NOT NULL,
  source_code TEXT NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (composition_id) REFERENCES compositions(id) ON DELETE CASCADE
);

CREATE INDEX idx_composition_versions_composition_id ON composition_versions(composition_id);
