//! `cardroi report` — portfolio summary + per-card P&L breakdown.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use comfy_table::Table;

use cardroi::analytics::portfolio::{AllocationEntry, AttributionEntry};
use cardroi::db::repository::Repository;
use cardroi::reports::export::{self, CardReportRow, PortfolioReport};

use super::roi::as_percent;

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// table | csv | json
    #[arg(long, default_value = "table")]
    format: String,
    /// Write to this file instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

pub fn run(repo: &Repository, args: ReportArgs) -> Result<()> {
    let report = export::build_report(repo).context("failed to build report")?;

    // Each render_* function returns exactly one trailing newline already,
    // so the output is `print!`-ed directly rather than `println!`-ed (which
    // would double it).
    let rendered = match args.format.as_str() {
        "table" => render_table(&report),
        "csv" => render_csv(&report)?,
        "json" => format!("{}\n", serde_json::to_string_pretty(&report)?),
        other => bail!("--format must be table, csv, or json, got {other:?}"),
    };

    match &args.output {
        Some(path) => {
            std::fs::write(path, &rendered)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        None => print!("{rendered}"),
    }

    Ok(())
}

/// CSV output is the per-card breakdown only — a single-row portfolio
/// summary doesn't fit a per-row tabular schema, so it's table/JSON-only.
fn render_csv(report: &PortfolioReport) -> Result<String> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(vec![]);
    for row in &report.cards {
        writer.serialize(row)?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|e| anyhow::anyhow!("csv writer error: {e}"))?;
    Ok(String::from_utf8(bytes)?)
}

fn render_table(report: &PortfolioReport) -> String {
    let p = &report.portfolio;
    let mut out = String::new();
    out.push_str("Portfolio\n");
    out.push_str(&format!(
        "  Holdings: {} ({} closed)\n",
        p.holding_count, p.closed_count
    ));
    out.push_str(&format!("  Total cost basis: {}\n", p.cost_basis));
    out.push_str(&format!(
        "  Capital still deployed (open): {}\n",
        p.open_cost_basis
    ));
    out.push_str(&format!("  Realized proceeds: {}\n", p.proceeds));
    out.push_str(&format!("  Realized P&L: {}\n", p.realized_pnl));
    out.push('\n');
    out.push_str(&card_table(&report.cards).to_string());
    out.push('\n');

    out.push_str("\nAllocation by card (currently-owned holdings; comp value where available, else cost basis)\n");
    out.push_str(&allocation_table(&report.allocation_by_card).to_string());
    out.push('\n');

    out.push_str("\nAllocation by set\n");
    out.push_str(&allocation_table(&report.allocation_by_set).to_string());
    out.push('\n');

    let c = &report.concentration;
    out.push_str("\nConcentration risk (HHI, over the by-card allocation)\n");
    out.push_str(&format!("  HHI: {}\n", c.hhi));
    match c.effective_positions {
        // `Decimal`'s `{:.2}` formatting truncates toward zero instead of
        // rounding (see commands::roi::as_percent's fix for the full
        // reasoning) - `round_dp` must run first. Didn't misfire on any
        // value exercised before now, but it's the same latent defect.
        Some(n) => out.push_str(&format!("  Effective positions: {:.2}\n", n.round_dp(2))),
        None => out.push_str("  Effective positions: n/a (nothing currently owned)\n"),
    }

    out.push_str("\nAttribution by player (all-time P&L)\n");
    out.push_str(&attribution_table(&report.attribution_by_player).to_string());
    out.push('\n');

    out.push_str("\nAttribution by sport (all-time P&L)\n");
    out.push_str(&attribution_table(&report.attribution_by_sport).to_string());
    out.push('\n');

    out
}

fn allocation_table(rows: &[AllocationEntry]) -> Table {
    let mut table = Table::new();
    table.set_header(vec!["Label", "Value", "Allocation %"]);
    for row in rows {
        table.add_row(vec![
            row.label.clone(),
            row.value.to_string(),
            as_percent(row.allocation_pct),
        ]);
    }
    table
}

fn attribution_table(rows: &[AttributionEntry]) -> Table {
    let mut table = Table::new();
    table.set_header(vec![
        "Label",
        "Holdings",
        "Closed",
        "Cost Basis",
        "Realized P&L",
        "Unrealized P&L",
        "Win %",
    ]);
    for row in rows {
        table.add_row(vec![
            row.label.clone(),
            row.pnl.holding_count.to_string(),
            row.pnl.closed_count.to_string(),
            row.pnl.cost_basis.to_string(),
            row.pnl.realized_pnl.to_string(),
            row.pnl.unrealized_pnl.to_string(),
            row.pnl
                .win_rate
                .map(as_percent)
                .unwrap_or_else(|| "n/a".to_string()),
        ]);
    }
    table
}

fn card_table(rows: &[CardReportRow]) -> Table {
    let mut table = Table::new();
    table.set_header(vec![
        "Card",
        "Holdings",
        "Closed",
        "Cost Basis",
        "Open Basis",
        "Proceeds",
        "Realized P&L",
        "Win %",
    ]);
    for row in rows {
        table.add_row(vec![
            row.card_name.clone(),
            row.holding_count.to_string(),
            row.closed_count.to_string(),
            row.cost_basis.to_string(),
            row.open_cost_basis.to_string(),
            row.proceeds.to_string(),
            row.realized_pnl.to_string(),
            row.win_rate_pct.clone().unwrap_or_default(),
        ]);
    }
    table
}
