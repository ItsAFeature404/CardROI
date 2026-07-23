//! Embedded schema migrations.
//!
//! Migrations are plain SQL, applied in order, tracked via `PRAGMA
//! user_version`. Each migration must be idempotent-safe to re-run against a
//! partially-applied database only in the sense that it runs inside a single
//! transaction that rolls back entirely on failure — there is no partial
//! application within one migration.

use rusqlite::Connection;

use crate::error::{CardRoiError, Result};

/// Ordered list of migrations. Index 0 is version 1, index 1 is version 2, etc.
/// Never edit a migration once released; append a new one instead.
const MIGRATIONS: &[&str] = &[
    MIGRATION_0001,
    MIGRATION_0002,
    MIGRATION_0003,
    MIGRATION_0004,
    MIGRATION_0005,
    MIGRATION_0006,
    MIGRATION_0007,
];

const MIGRATION_0001: &str = include_str!("migrations/0001_initial.sql");
const MIGRATION_0002: &str = include_str!("migrations/0002_appraisals.sql");
const MIGRATION_0003: &str = include_str!("migrations/0003_transaction_total_check.sql");
const MIGRATION_0004: &str = include_str!("migrations/0004_loss_transactions.sql");
const MIGRATION_0005: &str = include_str!("migrations/0005_composite_indexes.sql");
const MIGRATION_0006: &str = include_str!("migrations/0006_holding_images.sql");
const MIGRATION_0007: &str = include_str!("migrations/0007_holding_image_inline_storage.sql");

/// Applies any migrations newer than the database's current `user_version`.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if current_version as usize >= MIGRATIONS.len() {
        return Ok(());
    }

    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current_version as usize) {
        let target_version = (idx + 1) as u32;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)
            .map_err(|source| CardRoiError::Migration {
                version: target_version,
                source,
            })?;
        tx.pragma_update(None, "user_version", target_version)?;
        tx.commit()?;
        tracing::info!(version = target_version, "applied migration");
    }

    Ok(())
}
