//! Crate-wide error types.
//!
//! Library code returns [`CardRoiError`]; the CLI boundary (`main.rs` and
//! `commands/`) wraps these in `anyhow::Result` to attach user-facing context.

use thiserror::Error;

/// The single error type returned by all `cardroi` library APIs.
#[derive(Debug, Error)]
pub enum CardRoiError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("migration failed at version {version}: {source}")]
    Migration {
        version: u32,
        #[source]
        source: rusqlite::Error,
    },

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("{entity} not found (id={id})")]
    NotFound { entity: &'static str, id: i64 },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid money value {raw:?}: {reason}")]
    InvalidMoney { raw: String, reason: String },

    #[error("import error at row {row}: {message}")]
    Import { row: usize, message: String },

    #[error("{0}")]
    Other(String),
}

impl CardRoiError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn not_found(entity: &'static str, id: i64) -> Self {
        Self::NotFound { entity, id }
    }
}

pub type Result<T> = std::result::Result<T, CardRoiError>;
