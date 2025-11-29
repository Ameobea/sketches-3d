DROP TRIGGER IF EXISTS delete_thumbnails;

CREATE TRIGGER delete_thumbnails
AFTER DELETE ON composition_versions
FOR EACH ROW
WHEN OLD.thumbnail_url IS NOT NULL
BEGIN
  INSERT INTO deleted_thumbnails (url) VALUES (OLD.thumbnail_url);
END;
