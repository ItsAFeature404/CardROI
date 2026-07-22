//! The home screen: headline total value, total P&L against cost basis,
//! a top-movers strip, and the most recent ledger activity. Deliberately
//! nothing else - no IRR/TWR/HHI/full table here, per the
//! dashboard-restraint pattern every comparable app follows.
//!
//! "Total P&L vs. cost basis" stands in for a conventional dashboard's
//! day-over-day delta line: CardROI stores no periodic portfolio-value
//! snapshots, so a literal "+2.3% today" figure isn't something this
//! data model can honestly compute yet. This is the same cost-basis-
//! relative math the CLI already reports (`cardroi roi`), just surfaced
//! as the dashboard's headline delta instead of a time-based change.

use cardroi::analytics::roi::{RollupPnl, holding_pnl, portfolio_pnl};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{HoldingStatus, Money, TransactionType};
use chrono::NaiveDate;
use dioxus::prelude::*;
use rust_decimal::Decimal;

use crate::routes::Route;
use crate::web_bridge::WebBridge;

use super::format::{date, money, percent};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActivityItem {
    card_name: String,
    transaction_type: TransactionType,
    date: NaiveDate,
    total: Money,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MoverItem {
    holding_id: i64,
    card_name: String,
    unrealized_pnl: Money,
    unrealized_roi_pct: Option<Decimal>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DashboardData {
    pub(crate) rollup: RollupPnl,
    pub(crate) recent_activity: Vec<ActivityItem>,
    pub(crate) top_movers: Vec<MoverItem>,
}

/// Runs against the real `Repository` through the web bridge. One extra
/// query per recent-activity row and per priced owned holding
/// (resolving a card's display name) - the same N+1 shape
/// `analytics::roi::rollup` already accepts at this project's scale, not a
/// new performance risk.
pub(crate) fn load_dashboard_data(repo: &Repository) -> CardRoiResult<DashboardData> {
    let rollup = portfolio_pnl(repo)?;

    let mut transactions = repo.list_transactions(None, None, None)?;
    transactions.sort_by(|a, b| {
        a.transaction_date
            .cmp(&b.transaction_date)
            .then(a.id.cmp(&b.id))
    });
    let recent_activity = transactions
        .iter()
        .rev()
        .take(10)
        .map(|txn| -> CardRoiResult<ActivityItem> {
            let holding = repo.get_holding(txn.holding_id)?;
            let card = repo.get_card(holding.card_id)?;
            Ok(ActivityItem {
                card_name: card.display_name(),
                transaction_type: txn.transaction_type,
                date: txn.transaction_date,
                total: txn.total,
            })
        })
        .collect::<CardRoiResult<Vec<_>>>()?;

    let owned = repo.list_holdings(None, Some(HoldingStatus::Owned))?;
    let mut top_movers = Vec::new();
    for holding in &owned {
        let pnl = holding_pnl(repo, holding.id)?;
        if let Some(unrealized_pnl) = pnl.unrealized_pnl {
            let card = repo.get_card(holding.card_id)?;
            top_movers.push(MoverItem {
                holding_id: holding.id,
                card_name: card.display_name(),
                unrealized_pnl,
                unrealized_roi_pct: pnl.unrealized_roi_pct,
            });
        }
    }
    top_movers.sort_by_key(|m| std::cmp::Reverse(m.unrealized_pnl.cents().abs()));
    top_movers.truncate(5);

    Ok(DashboardData {
        rollup,
        recent_activity,
        top_movers,
    })
}

#[component]
pub fn Dashboard() -> Element {
    let bridge = use_context::<WebBridge>();
    let data = use_resource(move || {
        let bridge = bridge.clone();
        async move { bridge.run(load_dashboard_data).await }
    });

    match &*data.read() {
        None => rsx! {
            div { class: "p-8 text-text-secondary", "Loading..." }
        },
        Some(Err(err)) => rsx! {
            div { class: "p-8 text-loss", "Failed to load dashboard data: {err}" }
        },
        Some(Ok(data)) => rsx! {
            DashboardBody { data: data.clone() }
        },
    }
}

#[component]
fn DashboardBody(data: DashboardData) -> Element {
    if data.rollup.holding_count == 0 {
        return rsx! {
            div { class: "p-8 flex flex-col gap-8 max-w-4xl",
                h1 { class: "text-2xl font-semibold m-0", "Dashboard" }
                div { class: "flex flex-col gap-3 items-start",
                    p { class: "text-text-secondary m-0", "Nothing logged yet - buy your first card and this page fills in with your real numbers." }
                    Link {
                        to: Route::BuyRoute {},
                        class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none no-underline font-semibold cursor-pointer",
                        "Log your first buy"
                    }
                }
            }
        };
    }

    let rollup = &data.rollup;
    let total_value = rollup.open_cost_basis + rollup.unrealized_pnl;
    let total_pnl = rollup.realized_pnl + rollup.unrealized_pnl;
    let total_pnl_pct = total_pnl.ratio(rollup.cost_basis);
    let open_count = rollup.holding_count - rollup.closed_count;
    let is_gain = !total_pnl.is_negative();
    let pnl_class = if is_gain { "text-gain" } else { "text-loss" };
    let sign = if is_gain && !total_pnl.is_zero() {
        "+"
    } else {
        ""
    };

    rsx! {
        div { class: "p-8 flex flex-col gap-8 max-w-4xl",
            h1 { class: "text-2xl font-semibold m-0", "Dashboard" }

            div {
                p { class: "text-text-secondary text-sm m-0 mb-1", "Total value" }
                p { class: "data-numeral text-3xl m-0", "{money(total_value)}" }
                p { class: "data-numeral text-lg mt-1 mb-0 {pnl_class}",
                    "{sign}{money(total_pnl)}"
                    if let Some(pct) = total_pnl_pct {
                        " ({percent(pct)} since cost basis)"
                    }
                }
                if rollup.appraised_open_count < open_count {
                    p { class: "text-text-tertiary text-xs mt-2 mb-0",
                        "Unrealized P&L reflects user-supplied comps, not live market values - {rollup.appraised_open_count}/{open_count} open holdings priced"
                    }
                }
            }

            if !data.top_movers.is_empty() {
                div {
                    h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Top movers" }
                    div { class: "flex gap-4 flex-wrap",
                        for mover in data.top_movers.iter().cloned() {
                            MoverCard { mover }
                        }
                    }
                }
            }

            div {
                h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Recent activity" }
                if data.recent_activity.is_empty() {
                    p { class: "text-text-secondary m-0", "No transactions recorded yet." }
                } else {
                    div { class: "flex flex-col",
                        for item in data.recent_activity.iter().cloned() {
                            ActivityRow { item }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MoverCard(mover: MoverItem) -> Element {
    let is_gain = !mover.unrealized_pnl.is_negative();
    let class = if is_gain { "text-gain" } else { "text-loss" };
    rsx! {
        Link {
            to: Route::HoldingDetailRoute { id: mover.holding_id },
            class: "block bg-surface rounded-radius p-4 w-48 no-underline text-text-primary hover:bg-surface-elevated",
            p { class: "text-sm text-text-secondary m-0 mb-1 truncate", "{mover.card_name}" }
            p { class: "data-numeral text-lg m-0 {class}", "{money(mover.unrealized_pnl)}" }
            if let Some(pct) = mover.unrealized_roi_pct {
                p { class: "data-numeral text-xs m-0 {class}", "{percent(pct)}" }
            }
        }
    }
}

#[component]
fn ActivityRow(item: ActivityItem) -> Element {
    rsx! {
        div { class: "flex justify-between items-center py-3 border-b border-border",
            div {
                p { class: "m-0", "{item.card_name}" }
                p { class: "text-text-tertiary text-xs m-0 mt-1", "{item.transaction_type.as_str()} - {date(item.date)}" }
            }
            p { class: "data-numeral m-0", "{money(item.total)}" }
        }
    }
}
