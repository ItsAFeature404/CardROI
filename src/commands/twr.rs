//! `cardroi twr` — time-weighted return for a holding or the portfolio,
//! shown alongside IRR. TWR measures how well the investment itself
//! performed; IRR measures how well your own timing of buys/sells was.
//! They commonly diverge when capital is added or removed at a good or
//! bad time — that's not a bug in either number, see the note printed
//! below every result.

use anyhow::{Context, Result};
use clap::Args;
use rust_decimal::Decimal;

use cardroi::analytics::{irr, twr};
use cardroi::db::repository::Repository;

use super::roi::as_percent;

#[derive(Debug, Args)]
pub struct TwrArgs {
    #[arg(long = "holding-id")]
    holding_id: Option<i64>,
    /// Annualize over this many years, e.g. 1.5 (default: whole-period, unannualized)
    #[arg(long)]
    annualize: Option<String>,
}

pub fn run(repo: &Repository, args: TwrArgs) -> Result<()> {
    let years = args.annualize.as_deref().map(parse_years).transpose()?;

    match args.holding_id {
        Some(id) => {
            let twr_rate = twr::holding_twr(repo, id, years)
                .with_context(|| format!("failed to compute TWR for holding {id}"))?;
            println!("Holding #{id} TWR: {}%", as_percent(twr_rate));
            match irr::holding_irr(repo, id) {
                Ok(irr_rate) => println!("Holding #{id} IRR: {}%", as_percent(irr_rate)),
                Err(e) => println!("Holding #{id} IRR: n/a ({e})"),
            }
        }
        None => {
            let twr_rate =
                twr::portfolio_twr(repo, years).context("failed to compute portfolio TWR")?;
            println!(
                "Portfolio TWR (currently-owned holdings with a comp on record): {}%",
                as_percent(twr_rate)
            );
            match irr::portfolio_irr_closed_positions(repo) {
                Ok(irr_rate) => println!(
                    "Portfolio IRR (closed/sold positions only — a different scope than the TWR above): {}%",
                    as_percent(irr_rate)
                ),
                Err(e) => println!("Portfolio IRR (closed/sold positions only): n/a ({e})"),
            }
        }
    }
    println!(
        "(TWR isolates how well the investment performed; IRR reflects how well your own buy/sell timing and sizing was. They diverge when capital is added or removed at a good or bad time — neither number is \"wrong\" when that happens.)"
    );
    Ok(())
}

fn parse_years(s: &str) -> Result<Decimal> {
    s.parse::<Decimal>()
        .with_context(|| format!("invalid --annualize value: {s:?}, expected a number of years"))
}
