//! `cardroi holding` — CRUD and status transitions for physical holdings.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Subcommand;
use comfy_table::Table;

use cardroi::db::repository::Repository;
use cardroi::models::{Holding, HoldingEdit, HoldingStatus, Money, NewHolding};

#[derive(Debug, Subcommand)]
pub enum HoldingCommand {
    /// Create a new holding (one physical copy of a card)
    Add {
        #[arg(long = "card-id")]
        card_id: i64,
        #[arg(long)]
        serial: Option<String>,
        #[arg(long)]
        grade: Option<String>,
        #[arg(long = "grading-company")]
        grading_company: Option<String>,
        #[arg(long)]
        cert: Option<String>,
        /// Acquisition date, YYYY-MM-DD
        #[arg(long)]
        acquired: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List holdings, optionally filtered by card and/or status
    List {
        #[arg(long = "card-id")]
        card_id: Option<i64>,
        /// owned | sold | lost | damaged
        #[arg(long)]
        status: Option<String>,
    },
    /// Show a single holding
    Show { id: i64 },
    /// Delete a holding (fails if any transactions still reference it,
    /// unless --with-transactions is given)
    Delete {
        id: i64,
        /// Also delete every transaction recorded against this holding -
        /// permanent, real loss of ledger history, not just a status
        /// change. Only for a mistaken/test entry you're certain about.
        #[arg(long = "with-transactions")]
        with_transactions: bool,
    },
    /// Correct a holding's own physical/grading details after the fact
    /// (not its status or which card it is - see `holding mark-lost`/
    /// `mark-damaged`/`sell` for status changes). Omitting a flag leaves
    /// that field unchanged; passing an empty string clears it.
    Edit {
        id: i64,
        #[arg(long)]
        serial: Option<String>,
        #[arg(long)]
        grade: Option<String>,
        #[arg(long = "grading-company")]
        grading_company: Option<String>,
        #[arg(long)]
        cert: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Mark a holding as lost, recording a realized loss (a real
    /// disposition transaction, not just a status change)
    MarkLost {
        id: i64,
        /// YYYY-MM-DD, defaults to today
        #[arg(long)]
        date: Option<String>,
        /// Salvage/market value retained, if any (rare for a true loss —
        /// nothing physical remains with the owner). Defaults to 0.00.
        #[arg(long = "residual-value", allow_hyphen_values = true)]
        residual_value: Option<String>,
        /// Insurance or other reimbursement received, kept separate from
        /// residual value. Defaults to 0.00.
        #[arg(long = "insurance-recovery", allow_hyphen_values = true)]
        insurance_recovery: Option<String>,
        /// Free-text cause (e.g. "stolen", "lost in shipping") — for your
        /// own record only, never used in any calculation.
        #[arg(long)]
        cause: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Mark a holding as damaged, recording a realized loss (a real
    /// disposition transaction, not just a status change)
    MarkDamaged {
        id: i64,
        /// YYYY-MM-DD, defaults to today
        #[arg(long)]
        date: Option<String>,
        /// Salvage/market value the card retains despite the damage — an
        /// estimate you provide (damage discount varies enormously by
        /// rarity and damage type, so this is never computed for you).
        /// Defaults to 0.00 (total loss/no residual value).
        #[arg(long = "residual-value", allow_hyphen_values = true)]
        residual_value: Option<String>,
        /// Insurance or other reimbursement received, kept separate from
        /// residual value. Defaults to 0.00.
        #[arg(long = "insurance-recovery", allow_hyphen_values = true)]
        insurance_recovery: Option<String>,
        /// Free-text cause (e.g. "water damage", "crease in shipping") —
        /// for your own record only, never used in any calculation.
        #[arg(long)]
        cause: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
}

pub fn run(repo: &Repository, cmd: HoldingCommand) -> Result<()> {
    match cmd {
        HoldingCommand::Add {
            card_id,
            serial,
            grade,
            grading_company,
            cert,
            acquired,
            notes,
        } => {
            let acquired_date = acquired.map(|s| parse_date(&s)).transpose()?;
            let holding = repo
                .create_holding(&NewHolding {
                    card_id,
                    serial_number: serial,
                    grade,
                    grading_company,
                    cert_number: cert,
                    acquired_date,
                    notes,
                })
                .context("failed to create holding")?;
            print_holding(&holding);
        }
        HoldingCommand::List { card_id, status } => {
            let status = status.map(|s| parse_status(&s)).transpose()?;
            let holdings = repo
                .list_holdings(card_id, status)
                .context("failed to list holdings")?;
            print_table(&holdings);
        }
        HoldingCommand::Show { id } => {
            let holding = repo
                .get_holding(id)
                .with_context(|| format!("failed to fetch holding {id}"))?;
            print_holding(&holding);
        }
        HoldingCommand::Delete {
            id,
            with_transactions,
        } => {
            if with_transactions {
                repo.delete_holding_cascade(id)
                    .with_context(|| format!("failed to delete holding {id}"))?;
                println!("Deleted holding #{id} and all its transactions");
            } else {
                repo.delete_holding(id).with_context(|| {
                    format!(
                        "failed to delete holding {id} — it still has transactions referencing it; use --with-transactions to delete those too"
                    )
                })?;
                println!("Deleted holding #{id}");
            }
        }
        HoldingCommand::Edit {
            id,
            serial,
            grade,
            grading_company,
            cert,
            notes,
        } => {
            let current = repo
                .get_holding(id)
                .with_context(|| format!("failed to fetch holding {id}"))?;
            let edit = HoldingEdit {
                serial_number: apply_edit(serial, current.serial_number),
                grade: apply_edit(grade, current.grade),
                grading_company: apply_edit(grading_company, current.grading_company),
                cert_number: apply_edit(cert, current.cert_number),
                notes: apply_edit(notes, current.notes),
            };
            let updated = repo
                .update_holding(id, &edit)
                .with_context(|| format!("failed to update holding {id}"))?;
            println!("Updated holding #{id}");
            print_holding(&updated);
        }
        HoldingCommand::MarkLost {
            id,
            date,
            residual_value,
            insurance_recovery,
            cause,
            notes,
        } => {
            record_loss(
                repo,
                id,
                HoldingStatus::Lost,
                date,
                residual_value,
                insurance_recovery,
                cause,
                notes,
            )?;
        }
        HoldingCommand::MarkDamaged {
            id,
            date,
            residual_value,
            insurance_recovery,
            cause,
            notes,
        } => {
            record_loss(
                repo,
                id,
                HoldingStatus::Damaged,
                date,
                residual_value,
                insurance_recovery,
                cause,
                notes,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn record_loss(
    repo: &Repository,
    id: i64,
    status: HoldingStatus,
    date: Option<String>,
    residual_value: Option<String>,
    insurance_recovery: Option<String>,
    cause: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    let date = match date {
        Some(s) => parse_date(&s)?,
        None => chrono::Utc::now().date_naive(),
    };
    let residual_value = parse_money_or_zero(residual_value, "--residual-value")?;
    let insurance_recovery = parse_money_or_zero(insurance_recovery, "--insurance-recovery")?;

    let txn = repo
        .record_loss(
            id,
            status,
            date,
            residual_value,
            insurance_recovery,
            cause,
            notes,
        )
        .with_context(|| format!("failed to record loss for holding {id}"))?;

    println!(
        "Holding #{id} marked {}: realized proceeds {} (residual {} + insurance recovery {})",
        status.as_str(),
        txn.total,
        residual_value,
        insurance_recovery
    );
    Ok(())
}

/// Patch semantics for an editable optional-text field: the flag wasn't
/// passed at all -> keep the current value; passed with an empty string
/// -> clear it; passed with real text -> set it. Shared by every `edit`
/// command's optional-string flags (holding and transaction alike) so
/// "omit to leave unchanged" behaves identically everywhere.
pub(crate) fn apply_edit(flag: Option<String>, existing: Option<String>) -> Option<String> {
    match flag {
        None => existing,
        Some(s) if s.trim().is_empty() => None,
        Some(s) => Some(s),
    }
}

fn parse_money_or_zero(s: Option<String>, flag: &str) -> Result<Money> {
    match s {
        Some(s) => Money::from_str(&s).with_context(|| format!("invalid amount for {flag}: {s:?}")),
        None => Ok(Money::ZERO),
    }
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}

fn parse_status(s: &str) -> Result<HoldingStatus> {
    s.parse::<HoldingStatus>()
        .with_context(|| format!("invalid status {s:?}, expected owned|sold|lost|damaged"))
}

fn print_holding(holding: &Holding) {
    println!("Holding #{}: card {}", holding.id, holding.card_id);
    println!("  Status: {}", holding.status.as_str());
    if let Some(serial) = &holding.serial_number {
        println!("  Serial: {serial}");
    }
    if let Some(grade) = &holding.grade {
        println!(
            "  Grade: {grade} ({})",
            holding.grading_company.as_deref().unwrap_or("?")
        );
    }
    if let Some(date) = holding.acquired_date {
        println!("  Acquired: {date}");
    }
    if let Some(date) = holding.disposed_date {
        println!("  Disposed: {date}");
    }
    if let Some(notes) = &holding.notes {
        println!("  Notes: {notes}");
    }
}

fn print_table(holdings: &[Holding]) {
    let mut table = Table::new();
    table.set_header(vec!["ID", "Card", "Status", "Serial", "Grade"]);
    for holding in holdings {
        table.add_row(vec![
            holding.id.to_string(),
            holding.card_id.to_string(),
            holding.status.as_str().to_string(),
            holding.serial_number.clone().unwrap_or_default(),
            holding.grade.clone().unwrap_or_default(),
        ]);
    }
    println!("{table}");
}
