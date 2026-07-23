-- Adds inline (BLOB-in-row) storage for a photo's full-size bytes,
-- alongside the existing on-disk (file_path) storage native/CLI uses.
-- cardroi-web has no filesystem at all (its SQLite database file itself
-- is the only thing persisted, via IndexedDB) so a wasm-authored row
-- stores full bytes directly in full_data instead of writing a file
-- under some images_root. Exactly one of file_path/full_data is
-- populated per row, enforced with a CHECK rather than trusted to
-- application code alone (see db::repository::holding_images::PhotoStorage).
--
-- SQLite has no ALTER TABLE ... ADD CONSTRAINT and no way to drop a
-- NOT NULL, so this rebuilds the table in place, same 12-step procedure
-- as migrations 0003/0004. Nothing references holding_images as a
-- foreign key target.

PRAGMA foreign_keys = OFF;

CREATE TABLE holding_images_new (
    id                  INTEGER PRIMARY KEY,
    holding_id          INTEGER NOT NULL REFERENCES holdings(id) ON DELETE CASCADE,
    file_path           TEXT,
    full_data           BLOB,
    file_hash           TEXT NOT NULL,
    mime_type           TEXT NOT NULL,
    width               INTEGER NOT NULL,
    height              INTEGER NOT NULL,
    file_size_bytes     INTEGER NOT NULL,
    is_primary          INTEGER NOT NULL DEFAULT 0,
    position            INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    thumbnail_data      BLOB NOT NULL,
    CHECK ((file_path IS NOT NULL) <> (full_data IS NOT NULL))
);

INSERT INTO holding_images_new
    SELECT id, holding_id, file_path, NULL, file_hash, mime_type, width,
           height, file_size_bytes, is_primary, position, created_at,
           thumbnail_data
    FROM holding_images;

DROP TABLE holding_images;
ALTER TABLE holding_images_new RENAME TO holding_images;

CREATE INDEX idx_holding_images_holding_id ON holding_images(holding_id);
CREATE INDEX idx_holding_images_file_hash ON holding_images(file_hash);

-- At most one primary photo per holding, enforced at the schema level as
-- a backstop alongside Repository::set_primary_photo's own transactional
-- flip - belt and suspenders against a future direct-SQL bug.
CREATE UNIQUE INDEX idx_holding_images_one_primary
    ON holding_images(holding_id) WHERE is_primary = 1;

PRAGMA foreign_keys = ON;
