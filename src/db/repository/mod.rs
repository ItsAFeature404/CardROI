//! Repository layer: translates between SQL rows and domain models. Each
//! entity gets its own submodule with `impl Repository` blocks so this file
//! stays a thin coordinator.

mod appraisals;
mod cards;
mod holding_images;
mod holdings;
mod import;
mod sets;
mod transactions;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::Connection;
use rusqlite::types::{FromSqlError, Type};

pub use holding_images::PhotoStorage;
pub use import::{AcquisitionImportRow, ChecklistImportRow, ImportSummary};

/// Thin wrapper around a [`Connection`] exposing typed CRUD and P&L queries.
/// Not `Sync` (rusqlite connections aren't); a CLI invocation owns exactly
/// one for its lifetime.
pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Parses an RFC3339 timestamp string stored by the schema's
/// `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')` default into a UTC timestamp.
pub(crate) fn parse_timestamp(raw: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

/// Parses a `YYYY-MM-DD` date string.
pub(crate) fn parse_date(raw: String) -> rusqlite::Result<NaiveDate> {
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

/// Parses an enum stored as TEXT via its `FromStr` impl.
pub(crate) fn parse_enum<T: std::str::FromStr>(raw: String) -> rusqlite::Result<T> {
    raw.parse::<T>().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(FromSqlError::InvalidType),
        )
    })
}
