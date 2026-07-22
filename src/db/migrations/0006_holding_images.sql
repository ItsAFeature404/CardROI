-- Photographs of a physical card, attached to a holding. Full-size images
-- live on disk (content-addressed by file_hash, see
-- db::repository::holding_images) - only a small precomputed thumbnail is
-- stored here directly, since it's comfortably under SQLite's own
-- documented ~100KB internal-vs-external BLOB crossover, letting a
-- gallery view hydrate every thumbnail with one query.
CREATE TABLE holding_images (
    id                  INTEGER PRIMARY KEY,
    holding_id          INTEGER NOT NULL REFERENCES holdings(id) ON DELETE CASCADE,
    file_path           TEXT NOT NULL,
    file_hash           TEXT NOT NULL,
    mime_type           TEXT NOT NULL,
    width               INTEGER NOT NULL,
    height              INTEGER NOT NULL,
    file_size_bytes     INTEGER NOT NULL,
    is_primary          INTEGER NOT NULL DEFAULT 0,
    position            INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    thumbnail_data      BLOB NOT NULL
);

CREATE INDEX idx_holding_images_holding_id ON holding_images(holding_id);
CREATE INDEX idx_holding_images_file_hash ON holding_images(file_hash);

-- At most one primary photo per holding, enforced at the schema level as
-- a backstop alongside Repository::set_primary_photo's own transactional
-- flip - belt and suspenders against a future direct-SQL bug.
CREATE UNIQUE INDEX idx_holding_images_one_primary
    ON holding_images(holding_id) WHERE is_primary = 1;
