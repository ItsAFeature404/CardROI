//! Browser-local presentation preferences: the collector's display name,
//! and whether they've already been asked for it. Deliberately not
//! collection data — doesn't live in the SQLite database (see
//! `storage.rs`), isn't part of any export, and clearing it doesn't
//! touch a single card, holding, or transaction. Lives in the browser's
//! own `localStorage`, the same one-browser-one-copy locality as
//! everything else in this app.
//!
//! Raw `web-sys` rather than a third-party Dioxus storage crate: the
//! actual need (read once at mount, write once on submit-or-skip)
//! doesn't call for a reactive synced-signal abstraction.

use web_sys::window;

const NAME_KEY: &str = "cardroi:collector_name";
const NAME_PROMPTED_KEY: &str = "cardroi:collector_name_prompted";

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok()?
}

/// `None` if never set, if the browser has no localStorage available, or
/// if the collector was asked and skipped — all three read the same to
/// every caller, which is exactly right: an unset name should behave
/// identically regardless of which of those is why.
pub fn collector_name() -> Option<String> {
    let name = local_storage()?.get_item(NAME_KEY).ok().flatten()?;
    (!name.trim().is_empty()).then_some(name)
}

pub fn set_collector_name(name: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(NAME_KEY, name.trim());
    }
}

/// Whether the one-time "what should I call you" moment has already
/// fired in this browser — true whether the collector answered it or
/// explicitly skipped it, so it never asks twice either way.
pub fn has_prompted_for_name() -> bool {
    local_storage()
        .and_then(|s| s.get_item(NAME_PROMPTED_KEY).ok().flatten())
        .is_some()
}

pub fn mark_name_prompted() {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(NAME_PROMPTED_KEY, "true");
    }
}
