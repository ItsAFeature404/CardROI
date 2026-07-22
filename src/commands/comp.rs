//! `cardroi comp` — record and inspect comps (comparable sold listings) for
//! a holding: what the hobby actually uses to price a card, never a formal
//! third-party appraisal. Every value here is what the user typed in
//! themselves, timestamped like every other ledger entry.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Subcommand;
use comfy_table::Table;

use cardroi::db::repository::Repository;
use cardroi::models::{Appraisal, Money, NewAppraisal};

#[derive(Debug, Subcommand)]
pub enum CompCommand {
    /// Record a new comp (a user-supplied value as of a date, based on
    /// comparable sold listings)
    Add {
        #[arg(long = "holding-id")]
        holding_id: i64,
        /// Accepts a leading `-` so a negative value reaches our own
        /// validation with a clear reason, instead of clap's generic
        /// "unexpected argument" error for what looks like an unknown flag.
        #[arg(long, allow_hyphen_values = true)]
        value: String,
        /// Comp date, YYYY-MM-DD; defaults to today
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List every comp on record for a holding, oldest first
    List {
        #[arg(long = "holding-id")]
        holding_id: i64,
    },
    /// Show the most recent comp for a holding
    Latest {
        #[arg(long = "holding-id")]
        holding_id: i64,
    },
    /// Delete a comp
    Delete { id: i64 },
}

pub fn run(repo: &Repository, cmd: CompCommand) -> Result<()> {
    match cmd {
        CompCommand::Add {
            holding_id,
            value,
            date,
            source,
            notes,
        } => {
            let appraised_value = parse_money(&value)?;
            let appraised_date = match &date {
                Some(s) => parse_date(s)?,
                None => chrono::Utc::now().date_naive(),
            };
            let comp = repo
                .create_appraisal(&NewAppraisal {
                    holding_id,
                    appraised_value,
                    appraised_date,
                    source,
                    notes,
                })
                .with_context(|| format!("failed to record comp for holding {holding_id}"))?;
            print_comp(&comp);
        }
        CompCommand::List { holding_id } => {
            let comps = repo
                .list_appraisals_for_holding(holding_id)
                .with_context(|| format!("failed to list comps for holding {holding_id}"))?;
            print_table(&comps);
        }
        CompCommand::Latest { holding_id } => {
            let comp = repo
                .latest_appraisal_for_holding(holding_id)
                .with_context(|| format!("failed to fetch latest comp for holding {holding_id}"))?;
            match comp {
                Some(comp) => print_comp(&comp),
                None => println!("Holding #{holding_id} has no comps on record"),
            }
        }
        CompCommand::Delete { id } => {
            repo.delete_appraisal(id)
                .with_context(|| format!("failed to delete comp {id}"))?;
            println!("Deleted comp #{id}");
        }
    }
    Ok(())
}

fn parse_money(s: &str) -> Result<Money> {
    Money::from_str(s).with_context(|| format!("invalid amount for --value: {s:?}"))
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}

fn print_comp(comp: &Appraisal) {
    println!(
        "Comp #{} for holding #{}: {} as of {} (user-supplied value, not a live market price)",
        comp.id, comp.holding_id, comp.appraised_value, comp.appraised_date
    );
    if let Some(source) = &comp.source {
        println!("  Source: {source}");
    }
    if let Some(notes) = &comp.notes {
        println!("  Notes: {notes}");
    }
}

fn print_table(comps: &[Appraisal]) {
    let mut table = Table::new();
    table.set_header(vec!["ID", "Holding", "Value", "As of", "Source"]);
    for comp in comps {
        table.add_row(vec![
            comp.id.to_string(),
            comp.holding_id.to_string(),
            comp.appraised_value.to_string(),
            comp.appraised_date.to_string(),
            comp.source.clone().unwrap_or_default(),
        ]);
    }
    println!("{table}");
    println!("(all values are user-supplied comps as of the listed date, not live market prices)");
}
