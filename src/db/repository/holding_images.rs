//! Photographs of a physical card, attached to a holding. `add_photo` is
//! the single entry point every upload path calls (the LAN phone-scan
//! upload handler, the desktop file-picker fallback, and the web app's
//! browser upload alike) - no parallel validation/resize logic between
//! them. Full-size images are either content-addressed files on disk
//! under an `images_root` (native/CLI) or bytes stored directly in the
//! row (`cardroi-web`, which has no filesystem at all - see
//! `PhotoStorage`); only a small thumbnail ever lives in the row
//! regardless (see `models::holding_image` for why). All filesystem I/O
//! for this entity lives behind this module, same "no direct SQL/IO
//! elsewhere" discipline as the rest of the repository layer.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{GenericImageView, ImageEncoder, ImageReader};
use rusqlite::{OptionalExtension, Row, params};
use sha2::{Digest, Sha256};

use crate::error::{CardRoiError, Result};
use crate::models::HoldingImage;

use super::{Repository, parse_timestamp};

/// Full-size photos are capped to this on their long edge before storage,
/// plenty for a "what does this card look like" record, not a studio
/// scan; keeps on-disk size reasonable regardless of what the camera/
/// file-picker originally handed over.
const MAX_LONG_EDGE_DISK: u32 = 2000;
const JPEG_QUALITY_DISK: u8 = 85;
/// A browser has no real filesystem - the whole SQLite database (not
/// just photos) rides inside one IndexedDB blob with no size/quota
/// handling anywhere in `cardroi-web` today, so an inline-stored photo
/// is capped smaller than a disk-backed one. Still plenty of resolution
/// for "what does this card look like" reference viewing on a phone or
/// desktop screen.
const MAX_LONG_EDGE_INLINE: u32 = 1200;
const JPEG_QUALITY_INLINE: u8 = 78;
/// Thumbnail long edge - comfortably small enough to justify living as a
/// BLOB directly in the row (see the schema migration's doc comment).
const THUMBNAIL_LONG_EDGE: u32 = 300;
const THUMBNAIL_JPEG_QUALITY: u8 = 85;

/// Where a photo's full-size bytes physically live - chosen by the
/// caller, never inferred from `cfg(target_arch)` inside this module, so
/// the same decode/resize/encode logic runs unchanged either way.
#[derive(Clone, Copy)]
pub enum PhotoStorage<'a> {
    /// Native/CLI: full-size JPEG written to a content-addressed path
    /// under this directory; only the thumbnail lives in the row.
    Disk(&'a Path),
    /// `cardroi-web`: no filesystem exists in a browser sandbox - the
    /// full-size JPEG goes straight into the row's `full_data` column.
    /// No cross-holding content dedup here (unlike `Disk`, which shares
    /// one file across identical uploads) - v1 wasm scope is one photo
    /// per holding, so a hypothetical duplicate upload's extra bytes
    /// aren't worth the added complexity.
    Inline,
}

/// Resolves the on-disk path for a content hash, sharded two levels deep
/// by hex prefix (`ab/cd/abcd1234....jpg`) so the images directory never
/// becomes one enormous flat listing.
fn sharded_path(images_root: &Path, hash: &str) -> PathBuf {
    images_root
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.jpg"))
}

impl Repository {
    /// Decodes, resizes/re-encodes to JPEG, hashes, thumbnails, and
    /// stores an uploaded photo for `holding_id`. Under `PhotoStorage::Disk`,
    /// writes the file only if its hash isn't already on disk (dedup);
    /// the DB row is always inserted last, so a failed write never
    /// leaves a dangling row.
    pub fn add_photo(
        &self,
        holding_id: i64,
        image_bytes: &[u8],
        storage: PhotoStorage,
    ) -> Result<HoldingImage> {
        let (max_long_edge, jpeg_quality) = match storage {
            PhotoStorage::Disk(_) => (MAX_LONG_EDGE_DISK, JPEG_QUALITY_DISK),
            PhotoStorage::Inline => (MAX_LONG_EDGE_INLINE, JPEG_QUALITY_INLINE),
        };

        let img = ImageReader::new(Cursor::new(image_bytes))
            .with_guessed_format()
            .map_err(|e| CardRoiError::validation(format!("couldn't read this image: {e}")))?
            .decode()
            .map_err(|e| {
                CardRoiError::validation(format!(
                    "couldn't decode this image (try JPEG, PNG, or WebP): {e}"
                ))
            })?;

        let (orig_w, orig_h) = img.dimensions();
        let img = if orig_w.max(orig_h) > max_long_edge {
            let (w, h) = scaled_dimensions(orig_w, orig_h, max_long_edge);
            img.resize(w, h, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let (width, height) = img.dimensions();
        // JPEG has no alpha channel - convert to RGB8 unconditionally
        // rather than passing through the source's own color type (which
        // could be RGBA for a PNG/WebP upload and would fail to encode).
        let rgb = img.to_rgb8();

        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, jpeg_quality)
            .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .map_err(|e| CardRoiError::validation(format!("couldn't re-encode this image: {e}")))?;

        let file_hash = format!("{:x}", Sha256::digest(&encoded));

        let (file_path, full_data) = match storage {
            PhotoStorage::Disk(images_root) => {
                let file_path = sharded_path(images_root, &file_hash);
                if !file_path.exists() {
                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&file_path, &encoded)?;
                }
                let relative_path = file_path
                    .strip_prefix(images_root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                (Some(relative_path), None)
            }
            PhotoStorage::Inline => (None, Some(encoded.clone())),
        };

        let thumbnail = image::imageops::thumbnail(&rgb, THUMBNAIL_LONG_EDGE, THUMBNAIL_LONG_EDGE);
        let mut thumbnail_data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut thumbnail_data,
            THUMBNAIL_JPEG_QUALITY,
        )
        .write_image(
            thumbnail.as_raw(),
            thumbnail.width(),
            thumbnail.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| CardRoiError::validation(format!("couldn't generate a thumbnail: {e}")))?;

        let next_position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM holding_images WHERE holding_id = ?1",
            params![holding_id],
            |row| row.get(0),
        )?;
        let existing_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM holding_images WHERE holding_id = ?1",
            params![holding_id],
            |row| row.get(0),
        )?;
        let is_primary = existing_count == 0;

        self.conn.execute(
            "INSERT INTO holding_images (
                holding_id, file_path, full_data, file_hash, mime_type, width, height,
                file_size_bytes, is_primary, position, thumbnail_data
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                holding_id,
                file_path,
                full_data,
                file_hash,
                "image/jpeg",
                width,
                height,
                encoded.len() as i64,
                is_primary,
                next_position,
                thumbnail_data,
            ],
        )?;
        self.get_photo(self.conn.last_insert_rowid())
    }

    pub fn get_photo(&self, id: i64) -> Result<HoldingImage> {
        self.conn
            .query_row(
                "SELECT * FROM holding_images WHERE id = ?1",
                params![id],
                row_to_holding_image,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CardRoiError::not_found("holding_image", id)
                }
                other => other.into(),
            })
    }

    /// Reads a photo's full-size bytes (not the small in-row thumbnail) -
    /// for an enlarged/lightbox view, where the thumbnail's deliberately-
    /// small resolution would look blurry blown up. `storage` must match
    /// however the photo was originally written (`Disk` reads off disk,
    /// `Inline` returns the row's own `full_data` directly).
    pub fn get_photo_bytes(&self, id: i64, storage: PhotoStorage) -> Result<Vec<u8>> {
        let photo = self.get_photo(id)?;
        match storage {
            PhotoStorage::Disk(images_root) => {
                let file_path = photo.file_path.as_deref().ok_or_else(|| {
                    CardRoiError::validation(format!(
                        "holding_image {id} has no file_path - it was stored inline, not on disk"
                    ))
                })?;
                let path = images_root.join(file_path);
                std::fs::read(&path).map_err(|source| {
                    CardRoiError::validation(format!(
                        "couldn't read the photo file at {}: {source}",
                        path.display()
                    ))
                })
            }
            PhotoStorage::Inline => photo.full_data.ok_or_else(|| {
                CardRoiError::validation(format!(
                    "holding_image {id} has no full_data - it was stored on disk, not inline"
                ))
            }),
        }
    }

    /// All photos for a holding, ordered for display (primary first, then
    /// insertion order).
    pub fn list_photos_for_holding(&self, holding_id: i64) -> Result<Vec<HoldingImage>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM holding_images WHERE holding_id = ?1
             ORDER BY is_primary DESC, position ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![holding_id], row_to_holding_image)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Flips the primary flag to `image_id` (which must belong to
    /// `holding_id`) in one transaction, matching the schema's partial
    /// unique index.
    pub fn set_primary_photo(&self, holding_id: i64, image_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE holding_images SET is_primary = 0 WHERE holding_id = ?1",
            params![holding_id],
        )?;
        let affected = tx.execute(
            "UPDATE holding_images SET is_primary = 1 WHERE id = ?1 AND holding_id = ?2",
            params![image_id, holding_id],
        )?;
        if affected == 0 {
            return Err(CardRoiError::not_found("holding_image", image_id));
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes a single photo. If it was the primary, promotes the
    /// lowest-`position` remaining photo (if any) to primary. Unlinks the
    /// on-disk file only if no other row still references its hash -
    /// files are content-addressed and can be shared across rows. A
    /// no-op under `PhotoStorage::Inline` - the row's own `DELETE` above
    /// already frees the `full_data` blob, nothing on disk to clean up.
    pub fn delete_photo(&self, image_id: i64, storage: PhotoStorage) -> Result<()> {
        let photo = self.get_photo(image_id)?;

        self.conn.execute(
            "DELETE FROM holding_images WHERE id = ?1",
            params![image_id],
        )?;

        if photo.is_primary {
            let next_primary: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM holding_images WHERE holding_id = ?1
                     ORDER BY position ASC, id ASC LIMIT 1",
                    params![photo.holding_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = next_primary {
                self.conn.execute(
                    "UPDATE holding_images SET is_primary = 1 WHERE id = ?1",
                    params![id],
                )?;
            }
        }

        self.unlink_if_unreferenced(&photo.file_hash, storage)?;
        Ok(())
    }

    /// The GUI-only variant of `delete_holding` that also cleans up any
    /// photo files that become unreferenced. `delete_holding` itself
    /// (used by the CLI, which has no `images_root` context) stays
    /// untouched - this wraps it rather than changing its signature.
    /// `ON DELETE CASCADE` (foreign keys are enabled for every
    /// connection, see `db::connection::configure`) already removes the
    /// `holding_images` rows; this just handles the filesystem side.
    pub fn delete_holding_with_images(&self, holding_id: i64, storage: PhotoStorage) -> Result<()> {
        let hashes: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT DISTINCT file_hash FROM holding_images WHERE holding_id = ?1")?;
            let rows = stmt.query_map(params![holding_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        self.delete_holding(holding_id)?;

        for hash in hashes {
            self.unlink_if_unreferenced(&hash, storage)?;
        }
        Ok(())
    }

    /// The image-cleanup-aware variant of `delete_holding_cascade` - same
    /// relationship as `delete_holding_with_images` has to plain
    /// `delete_holding`. Used by the desktop GUI (which has an
    /// `images_root` context) for `Disk`-backed photos; the CLI has no
    /// GUI context to pass one, so it calls `delete_holding_cascade`
    /// directly instead. `cardroi-web` doesn't need this variant at all,
    /// even though it now has photos: `Inline` storage keeps a photo's
    /// full bytes in the very row `ON DELETE CASCADE` already removes,
    /// so plain `delete_holding_cascade` alone is enough - there's no
    /// on-disk file left to unlink.
    pub fn delete_holding_cascade_with_images(
        &self,
        holding_id: i64,
        storage: PhotoStorage,
    ) -> Result<()> {
        let hashes: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT DISTINCT file_hash FROM holding_images WHERE holding_id = ?1")?;
            let rows = stmt.query_map(params![holding_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        self.delete_holding_cascade(holding_id)?;

        for hash in hashes {
            self.unlink_if_unreferenced(&hash, storage)?;
        }
        Ok(())
    }

    /// A no-op under `PhotoStorage::Inline` - there's never a file on
    /// disk to unlink in the first place.
    fn unlink_if_unreferenced(&self, file_hash: &str, storage: PhotoStorage) -> Result<()> {
        let images_root = match storage {
            PhotoStorage::Disk(images_root) => images_root,
            PhotoStorage::Inline => return Ok(()),
        };
        let still_referenced: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM holding_images WHERE file_hash = ?1",
            params![file_hash],
            |row| row.get(0),
        )?;
        if still_referenced == 0 {
            let path = sharded_path(images_root, file_hash);
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

/// Scales `(w, h)` so its long edge is exactly `target_long_edge`,
/// preserving aspect ratio.
fn scaled_dimensions(w: u32, h: u32, target_long_edge: u32) -> (u32, u32) {
    if w >= h {
        let new_h = ((h as f64) * (target_long_edge as f64) / (w as f64)).round() as u32;
        (target_long_edge, new_h.max(1))
    } else {
        let new_w = ((w as f64) * (target_long_edge as f64) / (h as f64)).round() as u32;
        (new_w.max(1), target_long_edge)
    }
}

fn row_to_holding_image(row: &Row) -> rusqlite::Result<HoldingImage> {
    Ok(HoldingImage {
        id: row.get("id")?,
        holding_id: row.get("holding_id")?,
        file_path: row.get("file_path")?,
        full_data: row.get("full_data")?,
        file_hash: row.get("file_hash")?,
        mime_type: row.get("mime_type")?,
        width: row.get("width")?,
        height: row.get("height")?,
        // SQLite has no native unsigned type - stored (and bound on write,
        // above) as i64, same as every other integer column here. rusqlite
        // 0.40 dropped the u64 FromSql impl 0.32 provided (a correctness
        // fix upstream: it can't represent the top half of u64's range
        // faithfully) - read as i64 and cast, matching the write side
        // exactly. A byte count is never negative, so this is lossless in
        // practice.
        file_size_bytes: row.get::<_, i64>("file_size_bytes")? as u64,
        is_primary: row.get("is_primary")?,
        position: row.get("position")?,
        created_at: parse_timestamp(row.get("created_at")?)?,
        thumbnail_data: row.get("thumbnail_data")?,
    })
}
