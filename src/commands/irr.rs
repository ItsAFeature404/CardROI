//! `cardroi irr` — internal rate of return for a holding (closed - sold,
//! lost, or damaged - or still owned with a comp on record) or for
//! the whole portfolio's closed positions.

use anyhow::{Context, Result};
use clap::Args;
use rust_decimal::Decimal;

use cardroi::analytics::irr;
use cardroi::db::repository::Repository;
use cardroi::models::HoldingStatus;

use super::roi::as_percent;

#[derive(Debug, Args)]
pub struct IrrArgs {
    #[arg(long = "holding-id")]
    holding_id: Option<i64>,
}

/// XIRR always annualizes to a 1-year basis, so a real gain over a very
/// short holding period compounds to an enormous printed number -
/// correct math, easy to mistake for a bug. `>= 500%` is comfortably past
/// any real year-over-year card return, so it's a safe trigger for the
/// caveat without ever hiding or altering the real, computed rate.
fn extreme_rate_caveat(rate: Decimal) -> &'static str {
    if rate.abs() >= Decimal::from(5) {
        " (annualized over a short holding period - see `cardroi twr` for the un-annualized return)"
    } else {
        ""
    }
}

pub fn run(repo: &Repository, args: IrrArgs) -> Result<()> {
    match args.holding_id {
        Some(id) => {
            let rate = irr::holding_irr(repo, id)
                .with_context(|| format!("failed to compute IRR for holding {id}"))?;
            let holding = repo
                .get_holding(id)
                .with_context(|| format!("failed to fetch holding {id}"))?;
            if holding.status == HoldingStatus::Owned {
                let comp = repo
                    .latest_appraisal_for_holding(id)
                    .with_context(|| format!("failed to fetch comp for holding {id}"))?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "holding {id} has no comp despite holding_irr succeeding \
                             for an owned holding — this should be unreachable; please report it"
                        )
                    })?;
                println!(
                    "Holding #{id} IRR: {}% (still owned — uses the {} comp from {} as terminal value, a user-supplied estimate, not a live market value){}",
                    as_percent(rate),
                    comp.appraised_value,
                    comp.appraised_date,
                    extreme_rate_caveat(rate)
                );
            } else {
                println!(
                    "Holding #{id} IRR: {}%{}",
                    as_percent(rate),
                    extreme_rate_caveat(rate)
                );
            }
        }
        None => {
            let rate = irr::portfolio_irr_closed_positions(repo)
                .context("failed to compute portfolio IRR")?;
            println!(
                "Portfolio IRR (closed positions): {}%{}",
                as_percent(rate),
                extreme_rate_caveat(rate)
            );
        }
    }
    Ok(())
}
