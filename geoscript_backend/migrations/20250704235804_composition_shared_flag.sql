ALTER TABLE compositions ADD COLUMN is_shared BOOLEAN DEFAULT FALSE;
ALTER TABLE compositions ADD COLUMN is_featured BOOLEAN DEFAULT FALSE;

CREATE INDEX idx_compositions_is_shared ON compositions(is_shared);
CREATE INDEX idx_compositions_is_featured ON compositions(is_featured);
