//! `cardroi buy` — record an acquisition, creating a new holding.

use std::str::FromStr;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Args;

use cardroi::db::repository::Repository;
use cardroi::models::{Money, NewHolding, NewTransaction};

#[derive(Debug, Args)]
pub struct BuyArgs {
    #[arg(long = "card-id")]
    card_id: i64,
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
    /// How many identical holdings to create in one buy (each still an
    /// independently sellable Holding row). Incompatible with --serial and
    /// --cert, which must be unique per physical item.
    #[arg(long, default_value_t = 1)]
    quantity: u32,
    /// Acquisition date, YYYY-MM-DD; defaults to today
    #[arg(long)]
    date: Option<String>,
    #[arg(long)]
    serial: Option<String>,
    #[arg(long)]
    grade: Option<String>,
    #[arg(long = "grading-company")]
    grading_company: Option<String>,
    #[arg(long)]
    cert: Option<String>,
    #[arg(long)]
    counterparty: Option<String>,
    #[arg(long)]
    platform: Option<String>,
    #[arg(long = "external-ref")]
    external_ref: Option<String>,
    #[arg(long)]
    notes: Option<String>,
}

pub fn run(repo: &Repository, args: BuyArgs) -> Result<()> {
    if args.quantity == 0 {
        bail!("--quantity must be at least 1");
    }
    if args.quantity > 1 && (args.serial.is_some() || args.cert.is_some()) {
        bail!(
            "--serial and --cert identify a single physical item and cannot be used with --quantity > 1"
        );
    }

    let price = parse_money(&args.price, "--price")?;
    let fees = parse_money(&args.fees, "--fees")?;
    let shipping = parse_money(&args.shipping, "--shipping")?;
    let tax = parse_money(&args.tax, "--tax")?;
    let other_cost = parse_money(&args.other_cost, "--other-cost")?;
    let transaction_date = match &args.date {
        Some(s) => parse_date(s)?,
        None => chrono::Utc::now().date_naive(),
    };

    let mut total = Money::ZERO;
    let mut holding_ids = Vec::with_capacity(args.quantity as usize);

    for _ in 0..args.quantity {
        let (holding, txn) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: args.card_id,
                    serial_number: args.serial.clone(),
                    grade: args.grade.clone(),
                    grading_company: args.grading_company.clone(),
                    cert_number: args.cert.clone(),
                    acquired_date: Some(transaction_date),
                    notes: args.notes.clone(),
                },
                NewTransaction {
                    transaction_date,
                    price,
                    fees,
                    shipping,
                    tax,
                    other_cost,
                    currency: "USD".to_string(),
                    counterparty: args.counterparty.clone(),
                    platform: args.platform.clone(),
                    external_ref: args.external_ref.clone(),
                    notes: args.notes.clone(),
                    ..Default::default()
                },
            )
            .context("failed to record acquisition")?;
        total += txn.total;
        holding_ids.push(holding.id);
    }

    let ids = holding_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "Bought {} holding(s) [{ids}] for card {}, total {total}",
        holding_ids.len(),
        args.card_id
    );

    Ok(())
}

fn parse_money(s: &str, flag: &str) -> Result<Money> {
    Money::from_str(s).with_context(|| format!("invalid amount for {flag}: {s:?}"))
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}
