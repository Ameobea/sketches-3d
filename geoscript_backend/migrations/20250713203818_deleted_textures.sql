CREATE TABLE deleted_textures (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  url TEXT NOT NULL,
  thumbnail_url TEXT NOT NULL,
  deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER delete_textures
AFTER DELETE ON textures
FOR EACH ROW
BEGIN
  INSERT INTO deleted_textures (url, thumbnail_url) VALUES (OLD.url, OLD.thumbnail_url);
END;
