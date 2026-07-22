//! CardROI: local-first, precision investment portfolio management for
//! trading card collectors.

pub mod analytics;
pub mod db;
pub mod error;
pub mod models;
pub mod reports;

pub use error::{CardRoiError, Result};
