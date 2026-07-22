//! Registers CardROI's browser-local database backend: real SQLite
//! running in-browser via `sqlite-wasm-rs`'s FFI bindings, with
//! `sqlite-wasm-vfs`'s `RelaxedIdbVFS` persisting to IndexedDB (relaxed
//! durability, main-thread-safe, no COOP/COEP headers needed).
//!
//! `RelaxedIdbVFS` lives in a separate crate, `sqlite-wasm-vfs` -
//! `sqlite-wasm-rs` itself ships only the raw FFI bindings and a default
//! in-memory VFS. Once installed as SQLite's *default* VFS (the
//! `default_vfs: true` argument below), `Connection::open` picks up the
//! registered default VFS with no API difference - `src/db/repository/
//! *.rs` and the embedded migrations (`src/db/schema.rs`) need zero
//! wasm-specific code.
//!
//! **The pragma set differs from `db::connection::configure`'s native
//! set**: `journal_mode=WAL` and `synchronous=NORMAL` both fail with a
//! generic SQLite "SQL logic error" on this VFS - most likely because WAL
//! needs shared-memory VFS methods this still-experimental (the crate's
//! own description) implementation doesn't provide, and `synchronous`
//! tuning has no meaningful effect without it. Neither is a real loss
//! here: none of `sqlite-wasm-vfs`'s VFS backends support multiple
//! simultaneous connections at all, which is WAL's entire reason to
//! exist, and this VFS is explicitly "relaxed durability" by design
//! regardless of `synchronous`. `foreign_keys=ON` and `temp_store=MEMORY`
//! both work cleanly - `foreign_keys=ON` in particular is not optional to
//! lose: CardROI's schema relies on real FK constraints (cascading
//! deletes, RESTRICT-on-delete) that are load-bearing, unlike WAL/
//! `synchronous`, which are pure performance/durability tuning.

use rusqlite::Connection;
use sqlite_wasm_rs::WasmOsCallback;
use sqlite_wasm_vfs::relaxed_idb::{self, RelaxedIdbCfgBuilder};

/// The IndexedDB database name CardROI's browser build stores everything
/// under - distinct from the CLI's `cardroi.db` filename only to make it
/// obvious in browser DevTools which storage this is; IndexedDB has no
/// filesystem-path semantics, so the name itself carries no other
/// meaning.
const DB_NAME: &str = "cardroi-web.db";

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("failed to install the browser storage backend: {0}")]
    Vfs(#[from] relaxed_idb::RelaxedIdbError),
    #[error(transparent)]
    Db(#[from] cardroi::CardRoiError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Installs `RelaxedIdbVFS` as SQLite's default VFS, then opens (creating
/// if necessary), configures, and migrates the real CardROI database
/// through it. Async because IndexedDB itself is inherently async in a
/// browser - there is no way to hide this behind a synchronous call, so
/// `main.rs` shows a loading state until this resolves. Registering the
/// same VFS name twice is a documented no-op in `relaxed_idb::install`'s
/// own doc comment, so this is safe to call more than once, but only the
/// app's startup path should.
pub async fn open() -> Result<Connection, StorageError> {
    let cfg = RelaxedIdbCfgBuilder::new()
        .vfs_name("cardroi-relaxed-idb")
        .build();
    relaxed_idb::install::<WasmOsCallback>(&cfg, true).await?;

    let mut conn = Connection::open(DB_NAME)?;
    configure(&conn)?;
    cardroi::db::migrate(&mut conn)?;
    Ok(conn)
}

/// The wasm-appropriate subset of `cardroi::db::connection::configure`'s
/// pragmas - see this module's doc comment for which two are dropped and
/// why.
fn configure(conn: &Connection) -> Result<(), StorageError> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}
