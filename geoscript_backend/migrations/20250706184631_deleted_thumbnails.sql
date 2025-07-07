CREATE TABLE deleted_thumbnails (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  url TEXT NOT NULL,
  deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER delete_thumbnails
AFTER DELETE ON composition_versions
FOR EACH ROW
BEGIN
  INSERT INTO deleted_thumbnails (url) VALUES (OLD.thumbnail_url);
END;
