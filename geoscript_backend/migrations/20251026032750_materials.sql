CREATE TABLE materials (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  thumbnail_url TEXT,
  material_definition JSON NOT NULL,
  owner_id INTEGER NOT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP NOT NULL,
  is_shared INTEGER DEFAULT 0 NOT NULL,
  FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE,
  CHECK (LENGTH(name) <= 255),
  CHECK (LENGTH(material_definition) <= 500000),
  CHECK (LENGTH(material_definition) > 2)
)
