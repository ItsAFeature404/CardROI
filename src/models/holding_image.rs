//! The [`HoldingImage`] domain model — a photograph of a physical card
//! attached to a [`Holding`](super::holding::Holding). Every field here is
//! computed from the uploaded bytes (hash, dimensions, size, thumbnail)
//! rather than user-typed, so unlike [`Appraisal`](super::appraisal::Appraisal)
//! there is no parallel `NewHoldingImage` input struct to validate - the
//! single entry point is `Repository::add_photo`, which takes the raw
//! bytes directly (see `db::repository::holding_images`).

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct HoldingImage {
    pub id: i64,
    pub holding_id: i64,
    /// Relative to the images root (a sibling directory of wherever the
    /// database file itself resolves to) - never an absolute path, so the
    /// whole `{db file + images dir}` bundle stays portable across
    /// machines. `None` when this row's full-size bytes live in
    /// `full_data` instead (a browser has no filesystem to write to) -
    /// exactly one of the two is ever populated (see
    /// `db::repository::holding_images::PhotoStorage`).
    pub file_path: Option<String>,
    /// The full-size, normalized JPEG bytes, populated only for a row
    /// written without a filesystem (`cardroi-web`). `None` for every
    /// disk-backed row - this never bloats a native/CLI row's size.
    pub full_data: Option<Vec<u8>>,
    /// SHA-256 hex digest of the final, normalized (resized/re-encoded)
    /// bytes - used both for on-disk deduplication and as the immutable
    /// filename itself.
    pub file_hash: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub is_primary: bool,
    pub position: i64,
    pub created_at: DateTime<Utc>,
    /// A small, precomputed JPEG thumbnail stored directly in the row
    /// (the one deliberate BLOB-in-SQLite exception - comfortably under
    /// SQLite's own documented ~100KB internal-vs-external crossover),
    /// so a gallery view hydrates every thumbnail with one query instead
    /// of one filesystem read per photo.
    pub thumbnail_data: Vec<u8>,
}
