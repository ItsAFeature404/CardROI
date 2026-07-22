//! `cardroi` CLI entrypoint.

mod commands;

use std::path::PathBuf;

use anyhow::Context;
use cardroi::db::repository::Repository;
use clap::Parser;

/// CardROI: local-first, precision investment portfolio management for
/// trading card collectors.
#[derive(Debug, Parser)]
#[command(name = "cardroi", version, about)]
struct Cli {
    /// Path to the SQLite database file. Falls back to `CARDROI_DB`, then
    /// `./cardroi.db`.
    #[arg(long, global = true, env = "CARDROI_DB")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<commands::Commands>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(|| PathBuf::from("cardroi.db"));

    let conn = cardroi::db::open(&db_path)
        .with_context(|| format!("failed to open database at {}", db_path.display()))?;
    let repo = Repository::new(conn);

    if let Some(command) = cli.command {
        commands::dispatch(&repo, command)?;
    }

    Ok(())
}
