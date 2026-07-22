//! `cardroi sell` — record a disposition against an existing holding.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Args;

use cardroi::db::repository::Repository;
use cardroi::models::{Money, NewTransaction};

#[derive(Debug, Args)]
pub struct SellArgs {
    #[arg(long = "holding-id")]
    holding_id: i64,
    /// Accepts a leading `-` (e.g. a typo'd negative price) so it reaches
    /// our own validation with a clear reason, instead of clap's generic
    /// "unexpected argument" error for what looks like an unknown flag.
    #[arg(long, allow_hyphen_values = true)]
    price: String,
    #[arg(long, allow_hyphen_values = true, default_value = "0.00")]
    fees: String,
    #[arg(long, allow_hyphen_values = true, default_value = "0.00")]
    shipping: String,
    #[arg(long, allow_hyphen_values = true, default_value = "0.00")]
    tax: String,
    #[arg(
        long = "other-cost",
        allow_hyphen_values = true,
        default_value = "0.00"
    )]
    other_cost: String,
    /// Sale date, YYYY-MM-DD; defaults to today
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    counterparty: Option<String>,
    #[arg(long)]
    platform: Option<String>,
    #[arg(long = "external-ref")]
    external_ref: Option<String>,
    #[arg(long)]
    notes: Option<String>,
}

pub fn run(repo: &Repository, args: SellArgs) -> Result<()> {
    let price = parse_money(&args.price, "--price")?;
    let fees = parse_money(&args.fees, "--fees")?;
    let shipping = parse_money(&args.shipping, "--shipping")?;
    let tax = parse_money(&args.tax, "--tax")?;
    let other_cost = parse_money(&args.other_cost, "--other-cost")?;
    let transaction_date = match &args.date {
        Some(s) => parse_date(s)?,
        None => chrono::Utc::now().date_naive(),
    };

    let txn = repo
        .record_sale(NewTransaction {
            holding_id: args.holding_id,
            transaction_date,
            price,
            fees,
            shipping,
            tax,
            other_cost,
            currency: "USD".to_string(),
            counterparty: args.counterparty,
            platform: args.platform,
            external_ref: args.external_ref,
            notes: args.notes,
            ..Default::default()
        })
        .with_context(|| format!("failed to record sale of holding {}", args.holding_id))?;

    println!(
        "Sold holding #{}: net proceeds {}",
        args.holding_id, txn.total
    );

    Ok(())
}

fn parse_money(s: &str, flag: &str) -> Result<Money> {
    Money::from_str(s).with_context(|| format!("invalid amount for {flag}: {s:?}"))
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}
