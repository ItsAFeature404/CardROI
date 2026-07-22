//! CLI subcommand handlers. Each subcommand gets its own module.

pub mod buy;
pub mod card;
pub mod comp;
pub mod holding;
pub mod import;
pub mod irr;
pub mod report;
pub mod roi;
pub mod sell;
pub mod set;
pub mod transaction;
pub mod twr;
pub mod whatif;

use anyhow::Result;
use clap::Subcommand;

use cardroi::db::repository::Repository;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Manage card sets
    Set {
        #[command(subcommand)]
        command: set::SetCommand,
    },
    /// Manage card catalog entries
    Card {
        #[command(subcommand)]
        command: card::CardCommand,
    },
    /// Manage individual physical holdings
    Holding {
        #[command(subcommand)]
        command: holding::HoldingCommand,
    },
    /// Record an acquisition, creating a new holding
    Buy(buy::BuyArgs),
    /// Record a disposition against an existing holding
    Sell(sell::SellArgs),
    /// Realized P&L at holding, card, set, or portfolio scope
    Roi(roi::RoiArgs),
    /// Bulk-load acquisitions from CSV or JSON
    Import(import::ImportArgs),
    /// Portfolio summary + per-card P&L breakdown
    Report(report::ReportArgs),
    /// Internal rate of return for a holding (closed - sold, lost, or
    /// damaged - or open with a comp on record) or the portfolio's closed
    /// positions
    Irr(irr::IrrArgs),
    /// Manage comps (comparable sold listings) for a holding
    Comp {
        #[command(subcommand)]
        command: comp::CompCommand,
    },
    /// Correct an existing transaction's own fields after the fact
    Transaction {
        #[command(subcommand)]
        command: transaction::TransactionCommand,
    },
    /// Time-weighted return for a holding or portfolio, shown alongside IRR
    Twr(twr::TwrArgs),
    /// Simulate a hypothetical sale without writing anything
    Whatif(whatif::WhatifArgs),
}

pub fn dispatch(repo: &Repository, command: Commands) -> Result<()> {
    match command {
        Commands::Set { command } => set::run(repo, command),
        Commands::Card { command } => card::run(repo, command),
        Commands::Holding { command } => holding::run(repo, command),
        Commands::Buy(args) => buy::run(repo, args),
        Commands::Sell(args) => sell::run(repo, args),
        Commands::Roi(args) => roi::run(repo, args),
        Commands::Import(args) => import::run(repo, args),
        Commands::Report(args) => report::run(repo, args),
        Commands::Irr(args) => irr::run(repo, args),
        Commands::Comp { command } => comp::run(repo, command),
        Commands::Transaction { command } => transaction::run(repo, command),
        Commands::Twr(args) => twr::run(repo, args),
        Commands::Whatif(args) => whatif::run(repo, args),
    }
}
