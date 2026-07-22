//! Connection setup and pragma tuning.

use std::path::Path;

use rusqlite::Connection;

use crate::db::schema;
use crate::error::Result;

/// Opens (creating if necessary) the database at `path`, applies performance
/// pragmas, and runs any pending migrations.
pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    schema::migrate(&mut conn)?;
    Ok(conn)
}

/// Opens a private in-memory database. Used by tests and by callers that want
/// a scratch instance.
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    schema::migrate(&mut conn)?;
    Ok(conn)
}

/// Applies pragmas tuned for a single-writer, local-first CLI workload at
/// 10k-100k+ row scale: WAL for concurrent readers, NORMAL sync (safe under
/// WAL, much faster than FULL), and foreign key enforcement.
fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}
