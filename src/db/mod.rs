//! Database layer: connection management, schema migrations, and the
//! repository types that translate between SQL rows and domain models.

mod connection;
mod schema;

pub mod repository;

pub use connection::{open, open_in_memory};
// cardroi-web (Phase 3.5) needs migrate() reachable on its own, not
// bundled into open()'s all-in-one flow - it must install its browser
// storage VFS before any Connection is opened, so its bootstrap is
// necessarily open -> (wasm-appropriate pragmas) -> migrate as separate
// steps, unlike the CLI/desktop's single open() call. Re-exporting just
// the function, not `pub mod schema`, keeps the rest of that module's
// internals (the embedded migration SQL constants) out of the public API.
pub use schema::migrate;
