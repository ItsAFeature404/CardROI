//! `cardroi roi` — realized P&L at holding, card, set, or portfolio scope.

use anyhow::{Context, Result, bail};
use clap::Args;
use rust_decimal::Decimal;

use cardroi::analytics::roi::{self, HoldingPnl, RollupPnl};
use cardroi::db::repository::Repository;

#[derive(Debug, Args)]
pub struct RoiArgs {
    #[arg(long = "holding-id")]
    holding_id: Option<i64>,
    #[arg(long = "card-id")]
    card_id: Option<i64>,
    #[arg(long = "set-id")]
    set_id: Option<i64>,
    /// table | json
    #[arg(long, default_value = "table")]
    format: String,
}

pub fn run(repo: &Repository, args: RoiArgs) -> Result<()> {
    let scopes_given = [
        args.holding_id.is_some(),
        args.card_id.is_some(),
        args.set_id.is_some(),
    ]
    .into_iter()
    .filter(|b| *b)
    .count();
    if scopes_given > 1 {
        bail!("only one of --holding-id, --card-id, --set-id may be given");
    }
    if args.format != "table" && args.format != "json" {
        bail!("--format must be table or json, got {:?}", args.format);
    }

    if let Some(id) = args.holding_id {
        let pnl = roi::holding_pnl(repo, id)
            .with_context(|| format!("failed to compute ROI for holding {id}"))?;
        print_holding(&pnl, &args.format)?;
    } else if let Some(id) = args.card_id {
        let pnl = roi::card_pnl(repo, id)
            .with_context(|| format!("failed to compute ROI for card {id}"))?;
        print_rollup("Card", id, &pnl, &args.format)?;
    } else if let Some(id) = args.set_id {
        let pnl = roi::set_pnl(repo, id)
            .with_context(|| format!("failed to compute ROI for set {id}"))?;
        print_rollup("Set", id, &pnl, &args.format)?;
    } else {
        let pnl = roi::portfolio_pnl(repo).context("failed to compute portfolio ROI")?;
        print_portfolio(&pnl, &args.format)?;
    }

    Ok(())
}

fn print_holding(pnl: &HoldingPnl, format: &str) -> Result<()> {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(pnl)?);
        return Ok(());
    }

    println!("Holding #{} (card {})", pnl.holding_id, pnl.card_id);
    println!("  Status: {}", pnl.status.as_str());
    println!("  Cost basis: {}", pnl.cost_basis);
    println!("  Proceeds: {}", pnl.proceeds);
    match pnl.realized_pnl {
        Some(p) => println!("  Realized P&L: {p}"),
        None => println!("  Realized P&L: not yet realized (still owned)"),
    }
    match pnl.roi_pct {
        Some(pct) => println!("  ROI: {}%", as_percent(pct)),
        None => println!("  ROI: n/a"),
    }
    match (pnl.unrealized_pnl, pnl.unrealized_pnl_as_of) {
        (Some(p), Some(as_of)) => {
            println!(
                "  Unrealized P&L: {p} (as of {as_of}, user-supplied comp — not a live market value)"
            );
            if let Some(pct) = pnl.unrealized_roi_pct {
                println!("  Unrealized ROI: {}%", as_percent(pct));
            }
        }
        _ => println!("  Unrealized P&L: n/a (no comp on record)"),
    }
    if let Some(days) = pnl.holding_period_days {
        println!("  Holding period: {days} days");
    }
    Ok(())
}

fn print_rollup(label: &str, id: i64, pnl: &RollupPnl, format: &str) -> Result<()> {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(pnl)?);
        return Ok(());
    }
    println!("{label} #{id}");
    print_rollup_body(pnl);
    Ok(())
}

fn print_portfolio(pnl: &RollupPnl, format: &str) -> Result<()> {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(pnl)?);
        return Ok(());
    }
    println!("Portfolio");
    print_rollup_body(pnl);
    Ok(())
}

fn print_rollup_body(pnl: &RollupPnl) {
    println!(
        "  Holdings: {} ({} closed)",
        pnl.holding_count, pnl.closed_count
    );
    println!("  Total cost basis: {}", pnl.cost_basis);
    println!("  Capital still deployed (open): {}", pnl.open_cost_basis);
    println!("  Realized proceeds: {}", pnl.proceeds);
    println!("  Realized P&L: {}", pnl.realized_pnl);
    match pnl.win_rate {
        Some(rate) => println!("  Win rate: {}%", as_percent(rate)),
        None => println!("  Win rate: n/a (nothing closed yet)"),
    }
    let open_count = pnl.holding_count - pnl.closed_count;
    if pnl.appraised_open_count > 0 {
        println!(
            "  Unrealized P&L: {} (user-supplied comps, {}/{} open holdings priced)",
            pnl.unrealized_pnl, pnl.appraised_open_count, open_count
        );
    } else if open_count > 0 {
        println!("  Unrealized P&L: n/a (no open holdings have a comp on record)");
    }
}

pub(crate) fn as_percent(ratio: Decimal) -> String {
    // `Decimal`'s `{:.2}` formatting truncates toward zero rather than
    // rounding (verified directly: -36.5079...% formats to "-36.50", not
    // the correctly-rounded "-36.51") - `round_dp` must run first. This
    // is the single formatter behind every percentage the CLI prints
    // (ROI, win rate, allocation, IRR, TWR), so this one fix corrects
    // all of them at once.
    format!("{:.2}", (ratio * Decimal::from(100)).round_dp(2))
}

#[cfg(test)]
mod as_percent_tests {
    use super::as_percent;
    use rust_decimal::Decimal;

    #[test]
    fn rounds_half_up_instead_of_truncating() {
        // 23/63 = 0.36507936...%, which formatting-by-truncation would
        // wrongly render as "36.50" - this is the exact ratio this bug
        // produced on real portfolio data (a -$115/$315 unrealized loss).
        let ratio = Decimal::from(23) / Decimal::from(63);
        assert_eq!(as_percent(ratio), "36.51");
        assert_eq!(as_percent(-ratio), "-36.51");
    }

    #[test]
    fn exact_values_are_unaffected() {
        assert_eq!(as_percent(Decimal::ONE), "100.00");
        assert_eq!(as_percent(Decimal::ZERO), "0.00");
    }
}
