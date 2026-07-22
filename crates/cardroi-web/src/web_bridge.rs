//! Thin async wrapper around the real `Repository`. WASM in a browser is
//! single-threaded (there is no OS thread to spawn in the first place -
//! `sqlite-wasm-rs` itself isn't thread-safe either, compiled with
//! `-DSQLITE_THREADSAFE=0`), and `sqlite-wasm-vfs`'s IndexedDB access is
//! already async end to end via its own internal commit loop, so there is
//! no blocking I/O here to move off a UI thread. `WebBridge::run` exists
//! only to give screens a consistent "await a closure over `&Repository`"
//! calling convention, not because anything here needs a channel or a
//! worker thread.

use std::rc::Rc;

use cardroi::db::repository::Repository;

use crate::storage::{self, StorageError};

#[derive(Clone)]
pub struct WebBridge {
    repo: Rc<Repository>,
}

impl WebBridge {
    /// Installs the browser storage backend and opens the real database
    /// (`storage::open`), wrapping the resulting `Repository` for cheap
    /// `Clone`-into-context sharing across the app.
    pub async fn open() -> Result<Self, StorageError> {
        let conn = storage::open().await?;
        Ok(Self {
            repo: Rc::new(Repository::new(conn)),
        })
    }

    /// Runs `f` against the real `Repository`. Not actually async work
    /// itself (see module doc) - `async` only so screens can `.await`
    /// uniformly without caring whether a given call happens to be
    /// synchronous today.
    pub async fn run<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Repository) -> T,
    {
        f(&self.repo)
    }
}
