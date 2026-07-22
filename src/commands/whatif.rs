//! `cardroi whatif` — simulates a hypothetical disposition of a
//! currently-owned holding. Read-only: never calls `record_sale`, never
//! writes anything. Every output line is prefixed to make it unmistakable
//! from `roi`'s real, realized numbers (see `analytics::whatif`'s research
//! grounding on presenting hypothetical performance).

use std::str::FromStr;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Args;

use cardroi::analytics::whatif::{self, HypotheticalSale, PriceSource};
use cardroi::db::repository::Repository;
use cardroi::models::Money;

use super::roi::as_percent;

#[derive(Debug, Args)]
pub struct WhatifArgs {
    #[arg(long = "holding-id")]
    holding_id: i64,
    /// Hypothetical sale price. Exactly one of --price / --at-comp is required.
    #[arg(long, allow_hyphen_values = true)]
    price: Option<String>,
    /// Use the holding's latest comp as the hypothetical price
    #[arg(long = "at-comp")]
    at_comp: bool,
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
    /// Hypothetical sale date, YYYY-MM-DD; defaults to today
    #[arg(long)]
    date: Option<String>,
    /// table | json
    #[arg(long, default_value = "table")]
    format: String,
}

pub fn run(repo: &Repository, args: WhatifArgs) -> Result<()> {
    if args.price.is_some() == args.at_comp {
        bail!("exactly one of --price or --at-comp is required");
    }
    if args.format != "table" && args.format != "json" {
        bail!("--format must be table or json, got {:?}", args.format);
    }

    let fees = parse_money(&args.fees, "--fees")?;
    let shipping = parse_money(&args.shipping, "--shipping")?;
    let tax = parse_money(&args.tax, "--tax")?;
    let other_cost = parse_money(&args.other_cost, "--other-cost")?;
    let sale_date = match &args.date {
        Some(s) => parse_date(s)?,
        None => chrono::Utc::now().date_naive(),
    };

    let (price, price_source) = if let Some(p) = &args.price {
        (parse_money(p, "--price")?, PriceSource::Given)
    } else {
        let comp = repo
            .latest_appraisal_for_holding(args.holding_id)
            .with_context(|| format!("failed to fetch latest comp for holding {}", args.holding_id))?
            .with_context(|| {
                format!(
                    "holding {} has no comp on record; use --price instead or record one with `cardroi comp add`",
                    args.holding_id
                )
            })?;
        (
            comp.appraised_value,
            PriceSource::LatestAppraisal {
                appraised_date: comp.appraised_date,
            },
        )
    };

    let sale = HypotheticalSale {
        price,
        fees,
        shipping,
        tax,
        other_cost,
        date: sale_date,
        price_source,
    };

    let result = whatif::holding_whatif(repo, args.holding_id, sale)
        .with_context(|| format!("failed to compute what-if for holding {}", args.holding_id))?;

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!(
        "HYPOTHETICAL — holding #{} was NOT sold; nothing was written",
        result.holding_id
    );
    println!(
        "  Assumed: sell at {} on {} ({})",
        result.assumed_price, result.assumed_sale_date, result.assumed_price_source
    );
    println!(
        "  Assumed costs: fees {}, shipping {}, tax {}, other {}",
        result.assumed_fees, result.assumed_shipping, result.assumed_tax, result.assumed_other_cost
    );
    println!("  Cost basis: {}", result.cost_basis);
    println!(
        "  IF sold: net proceeds {}, hypothetical realized P&L {}",
        result.hypothetical_net_proceeds, result.hypothetical_realized_pnl
    );
    match result.hypothetical_roi_pct {
        Some(pct) => println!("  Hypothetical ROI: {}%", as_percent(pct)),
        None => println!("  Hypothetical ROI: n/a"),
    }
    match result.hypothetical_irr_pct {
        Some(pct) => println!("  Hypothetical IRR: {}%", as_percent(pct)),
        None => println!("  Hypothetical IRR: n/a (no defined rate for this cash-flow pattern)"),
    }

    Ok(())
}

fn parse_money(s: &str, flag: &str) -> Result<Money> {
    Money::from_str(s).with_context(|| format!("invalid amount for {flag}: {s:?}"))
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::from_str(s).with_context(|| format!("invalid date {s:?}, expected YYYY-MM-DD"))
}
