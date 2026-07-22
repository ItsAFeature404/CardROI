//! Portfolio + per-card P&L report, exported as table/CSV/JSON.

use serde::Serialize;

use crate::analytics::portfolio::{self, AllocationEntry, AttributionEntry, Concentration};
use crate::analytics::roi::{self, RollupPnl};
use crate::db::repository::Repository;
use crate::error::Result;
use crate::models::Money;

/// One row of the per-card breakdown. Deliberately flat (no nested struct)
/// so it serializes identically well as a CSV row or a JSON object — the
/// `csv` crate's serde support does not handle `#[serde(flatten)]`.
#[derive(Debug, Clone, Serialize)]
pub struct CardReportRow {
    pub card_id: i64,
    pub card_name: String,
    pub holding_count: usize,
    pub closed_count: usize,
    pub cost_basis: Money,
    pub open_cost_basis: Money,
    pub proceeds: Money,
    pub realized_pnl: Money,
    /// Formatted as e.g. `"66.67"`, not a fraction, and empty when nothing
    /// has sold yet — keeps the CSV column plain text.
    pub win_rate_pct: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioReport {
    pub portfolio: RollupPnl,
    pub cards: Vec<CardReportRow>,
    /// Current portfolio composition (owned holdings only) by card and by
    /// set, each weighted by latest appraised value where available, else
    /// cost basis. See `analytics::portfolio` for the exact convention.
    pub allocation_by_card: Vec<AllocationEntry>,
    pub allocation_by_set: Vec<AllocationEntry>,
    /// Concentration risk (HHI) over `allocation_by_card`.
    pub concentration: Concentration,
    /// All-time P&L attribution, reusing the same rollup as `cards` above,
    /// just grouped differently.
    pub attribution_by_player: Vec<AttributionEntry>,
    pub attribution_by_sport: Vec<AttributionEntry>,
}

pub fn build_report(repo: &Repository) -> Result<PortfolioReport> {
    let portfolio = roi::portfolio_pnl(repo)?;
    let cards = repo.list_cards(None)?;

    let mut rows = Vec::with_capacity(cards.len());
    for card in &cards {
        let pnl = roi::card_pnl(repo, card.id)?;
        rows.push(CardReportRow {
            card_id: card.id,
            card_name: card.display_name(),
            holding_count: pnl.holding_count,
            closed_count: pnl.closed_count,
            cost_basis: pnl.cost_basis,
            open_cost_basis: pnl.open_cost_basis,
            proceeds: pnl.proceeds,
            realized_pnl: pnl.realized_pnl,
            // `Decimal`'s `{:.2}` formatting truncates toward zero rather
            // than rounding (verified directly: -36.5079...% formats to
            // "-36.50", not the correctly-rounded "-36.51") - `round_dp`
            // must run first. Same fix as `commands::roi::as_percent`,
            // duplicated here since `commands` is binary-only and not
            // part of this library crate.
            win_rate_pct: pnl.win_rate.map(|rate| {
                format!(
                    "{:.2}",
                    (rate * rust_decimal::Decimal::from(100)).round_dp(2)
                )
            }),
        });
    }

    let allocation_by_card = portfolio::allocation_by_card(repo)?;
    let concentration = portfolio::concentration_by_card(repo)?;

    Ok(PortfolioReport {
        portfolio,
        cards: rows,
        allocation_by_set: portfolio::allocation_by_set(repo)?,
        allocation_by_card,
        concentration,
        attribution_by_player: portfolio::attribution_by_player(repo)?,
        attribution_by_sport: portfolio::attribution_by_sport(repo)?,
    })
}
