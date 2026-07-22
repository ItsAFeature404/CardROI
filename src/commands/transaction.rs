//! `cardroi transaction` — corrections to a ledger entry that already
//! exists (a wrong price, a typo'd date), never a way to create one or
//! change its type/holding. New transactions come from `buy`/`sell`/
//! `holding mark-lost`/`mark-damaged`, which each stay the single entry
//! point for their own transaction type.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Subcommand;

use cardroi::db::repository::Repository;
use cardroi::models::{Money, Transaction, TransactionEdit};

use super::holding::apply_edit;

// `Edit`'s many optional flags make it much larger than `Show { id: i64 }`
// - clippy's size-difference concern is about hot-path storage of enum
// values in bulk, which doesn't apply here: one value is constructed per
// CLI invocation from parsed args, never stored in a collection.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum TransactionCommand {
    /// Show a single transaction
    Show { id: i64 },
    /// Correct an existing transaction's own fields (not its type or
    /// which holding it belongs to). Omitting a flag leaves that field
    /// unchanged; passing an empty string clears an optional text field.
    Edit {
        id: i64,
        /// YYYY-MM-DD
        #[arg(long)]
        date: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        price: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        fees: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        shipping: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        tax: Option<String>,
        #[arg(long = "other-cost", allow_hyphen_values = true)]
        other_cost: Option<String>,
        #[arg(long)]
        counterparty: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long = "external-ref")]
        external_ref: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
}

pub fn run(repo: &Repository, cmd: TransactionCommand) -> Result<()> {
    match cmd {
        TransactionCommand::Show { id } => {
            let txn = repo
                .get_transaction(id)
                .with_context(|| format!("failed to fetch transaction {id}"))?;
            print_transaction(&txn);
        }
        TransactionCommand::Edit {
            id,
            date,
            price,
            fees,
            shipping,
            tax,
            other_cost,
            counterparty,
            platform,
            external_ref,
            notes,
        } => {
            let current = repo
                .get_transaction(id)
                .with_context(|| format!("failed to fetch transaction {id}"))?;
            let edit = TransactionEdit {
                transaction_date: match date {
                    Some(s) => parse_date(&s)?,
                    None => current.transaction_date,
                },
                price: parse_money_or_keep(price, current.price, "--price")?,
                fees: parse_money_or_keep(fees, current.fees, "--fees")?,
                shipping: parse_money_or_keep(shipping, current.shipping, "--shipping")?,
                tax: parse_money_or_keep(tax, current.tax, "--tax")?,
                other_cost: parse_money_or_keep(other_cost, current.other_cost, "--other-cost")?,
                currency: current.currency,
                counterparty: apply_edit(counterparty, current.counterparty),
                platform: apply_edit(platform, current.platform),
                external_ref: apply_edit(external_ref, current.external_ref),
                notes: apply_edit(notes, current.notes),
            };
            let updated = repo
                .update_transaction(id, &edit)
                .with_context(|| format!("failed to update transaction {id}"))?;
            println!("Updated transaction #{id}");
            print_transaction(&updated);
        }
    }
    Ok(())
}

fn parse_money_or_keep(s: Option<String>, existing: Money, flag: &str) -> Result<Money> {
    match s {
        Some(s) => Money::from_str(&s).with_context(|| format!("invalid amount for {flag}: {s:?}")),
        None => Ok(existing),
    }
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}

fn print_transaction(txn: &Transaction) {
    println!(
        "Transaction #{} for holding #{}: {} ({}) on {}",
        txn.id,
        txn.holding_id,
        txn.transaction_type.as_str(),
        txn.total,
        txn.transaction_date
    );
    println!(
        "  Price {}, fees {}, shipping {}, tax {}, other {}",
        txn.price, txn.fees, txn.shipping, txn.tax, txn.other_cost
    );
    if let Some(counterparty) = &txn.counterparty {
        println!("  Counterparty: {counterparty}");
    }
    if let Some(platform) = &txn.platform {
        println!("  Platform: {platform}");
    }
    if let Some(external_ref) = &txn.external_ref {
        println!("  Reference: {external_ref}");
    }
    if let Some(notes) = &txn.notes {
        println!("  Notes: {notes}");
    }
}
