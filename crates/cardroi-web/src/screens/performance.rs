//! The Advanced performance view: IRR/TWR for the whole portfolio or one
//! holding, kept off the Dashboard entirely and reached one click deeper
//! here - dashboards read better capped at headline/delta/movers/activity,
//! never IRR/TWR/HHI. The plain realized/unrealized ROI% is shown first
//! either way; IRR/TWR sit behind an explicit "Show advanced" toggle so a
//! collector who doesn't care about GIPS-style return math never has to
//! see it.
//!
//! Calls `analytics::irr::{holding_irr, portfolio_irr_closed_positions}`
//! and `analytics::twr::{holding_twr, portfolio_twr}` directly through the
//! web bridge - the exact same functions `cardroi irr`/`cardroi twr`
//! call, including the same comp-as-terminal-value convention for open
//! positions and the same "insufficient comps"/"no sign change" error
//! wording on failure.

use cardroi::analytics::{irr, roi, twr};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::Money;
use dioxus::prelude::*;
use rust_decimal::Decimal;

use crate::components::form_field::FormField;
use crate::components::holding_picker::{HoldingOption, HoldingPicker};
use crate::web_bridge::WebBridge;

use super::format::{money, percent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Portfolio,
    Holding,
}

#[derive(Clone, Debug, PartialEq)]
struct PerformanceData {
    cost_basis: Money,
    realized_pnl: Option<Money>,
    realized_roi_pct: Option<Decimal>,
    unrealized_pnl: Option<Money>,
    unrealized_roi_pct: Option<Decimal>,
    win_rate: Option<Decimal>,
    irr: Result<Decimal, String>,
    twr: Result<Decimal, String>,
}

fn load_performance(
    holding_id: Option<i64>,
    annualize_years: Option<Decimal>,
    repo: &Repository,
) -> CardRoiResult<PerformanceData> {
    match holding_id {
        Some(id) => {
            let pnl = roi::holding_pnl(repo, id)?;
            let irr_result = irr::holding_irr(repo, id).map_err(|e| e.to_string());
            let twr_result = twr::holding_twr(repo, id, annualize_years).map_err(|e| e.to_string());
            Ok(PerformanceData {
                cost_basis: pnl.cost_basis,
                realized_pnl: pnl.realized_pnl,
                realized_roi_pct: pnl.roi_pct,
                unrealized_pnl: pnl.unrealized_pnl,
                unrealized_roi_pct: pnl.unrealized_roi_pct,
                win_rate: None,
                irr: irr_result,
                twr: twr_result,
            })
        }
        None => {
            let rollup = roi::portfolio_pnl(repo)?;
            let irr_result = irr::portfolio_irr_closed_positions(repo).map_err(|e| e.to_string());
            let twr_result = twr::portfolio_twr(repo, annualize_years).map_err(|e| e.to_string());
            Ok(PerformanceData {
                cost_basis: rollup.cost_basis,
                realized_pnl: Some(rollup.realized_pnl),
                realized_roi_pct: rollup.realized_pnl.ratio(rollup.cost_basis),
                unrealized_pnl: Some(rollup.unrealized_pnl),
                unrealized_roi_pct: rollup.unrealized_pnl.ratio(rollup.open_cost_basis),
                win_rate: rollup.win_rate,
                irr: irr_result,
                twr: twr_result,
            })
        }
    }
}

#[component]
pub fn Performance() -> Element {
    let bridge = use_context::<WebBridge>();
    let mut scope = use_signal(|| Scope::Portfolio);
    let selected_holding = use_signal(|| None::<HoldingOption>);
    let show_advanced = use_signal(|| false);
    let annualize_input = use_signal(String::new);

    let holding_id = move || match scope() {
        Scope::Portfolio => None,
        Scope::Holding => selected_holding().map(|h| h.holding_id),
    };

    let data = use_resource(move || {
        let bridge = bridge.clone();
        let holding_id = holding_id();
        let annualize_input = annualize_input();
        async move {
            let years = if annualize_input.trim().is_empty() {
                None
            } else {
                annualize_input.trim().parse::<Decimal>().ok()
            };
            bridge
                .run(move |repo| load_performance(holding_id, years, repo))
                .await
        }
    });

    rsx! {
        div { class: "p-8 flex flex-col gap-6 max-w-3xl",
            h1 { class: "text-2xl font-semibold m-0", "Performance" }

            div { class: "flex gap-2",
                button {
                    class: if scope() == Scope::Portfolio { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| scope.set(Scope::Portfolio),
                    "Portfolio"
                }
                button {
                    class: if scope() == Scope::Holding { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| scope.set(Scope::Holding),
                    "One holding"
                }
            }

            if scope() == Scope::Holding {
                div {
                    label { class: "text-text-secondary text-xs", "Holding" }
                    HoldingPicker { selected: selected_holding, status_filter: None }
                }
            }

            if scope() == Scope::Portfolio || selected_holding().is_some() {
                match &*data.read() {
                    None => rsx! {
                        div { class: "text-text-secondary", "Loading..." }
                    },
                    Some(Err(err)) => rsx! {
                        div { class: "text-loss", "Failed to load performance data: {err}" }
                    },
                    Some(Ok(perf)) => rsx! {
                        PerformanceBody {
                            perf: perf.clone(),
                            show_advanced,
                            annualize_input,
                            is_holding_scope: scope() == Scope::Holding,
                        }
                    },
                }
            }
        }
    }
}

/// XIRR always annualizes to a 1-year basis, so any real gain over a very
/// short holding period compounds to an enormous number - correct math,
/// but the raw percentage alone reads as a bug to someone who isn't
/// already fluent in annualized-return conventions. `>= 500%` (a ratio of
/// `5`) is comfortably past what any real year-over-year card return
/// looks like, so it's a safe trigger for the caveat without ever hiding
/// the real, computed number.
fn is_extreme_annualized_rate(rate: Decimal) -> bool {
    rate.abs() >= Decimal::from(5)
}

#[component]
fn PerformanceBody(
    perf: PerformanceData,
    show_advanced: Signal<bool>,
    annualize_input: Signal<String>,
    is_holding_scope: bool,
) -> Element {
    let mut show_advanced = show_advanced;
    rsx! {
        div { class: "flex flex-col gap-6",
            div {
                p { class: "text-text-secondary text-sm m-0 mb-1", "Cost basis" }
                p { class: "data-numeral text-2xl m-0", "{money(perf.cost_basis)}" }

                if let Some(realized) = perf.realized_pnl {
                    p {
                        class: if realized.is_negative() { "data-numeral text-lg mt-1 mb-0 text-loss" } else { "data-numeral text-lg mt-1 mb-0 text-gain" },
                        "realized {money(realized)}"
                        if let Some(pct) = perf.realized_roi_pct {
                            " ({percent(pct)})"
                        }
                    }
                }
                if let Some(unrealized) = perf.unrealized_pnl {
                    p {
                        class: if unrealized.is_negative() { "data-numeral text-sm mt-1 mb-0 text-loss" } else { "data-numeral text-sm mt-1 mb-0 text-gain" },
                        "unrealized {money(unrealized)}"
                        if let Some(pct) = perf.unrealized_roi_pct {
                            " ({percent(pct)})"
                        }
                    }
                }
                if let Some(win_rate) = perf.win_rate {
                    p { class: "text-text-secondary text-sm mt-1 mb-0", "Win rate: {percent(win_rate)}" }
                }
            }

            button {
                class: "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer self-start",
                onclick: move |_| show_advanced.set(!show_advanced()),
                if show_advanced() { "Hide advanced" } else { "Show advanced (IRR/TWR)" }
            }

            if show_advanced() {
                div { class: "flex flex-col gap-4 p-4 bg-surface rounded-radius",
                    div { class: "max-w-xs",
                        FormField { label: "Annualize over (years, optional)", value: annualize_input, placeholder: "e.g. 1.5" }
                    }

                    div {
                        p { class: "text-text-secondary text-sm m-0 mb-1",
                            if is_holding_scope { "Holding IRR" } else { "Portfolio IRR (closed/sold positions only)" }
                        }
                        match &perf.irr {
                            Ok(rate) => rsx! {
                                p { class: "data-numeral text-xl m-0", "{percent(*rate)}" }
                                if is_extreme_annualized_rate(*rate) {
                                    p { class: "text-text-tertiary text-xs mt-1 mb-0",
                                        "IRR annualizes to a 1-year basis - a real gain over a very short holding period compounds to a number this large on paper. TWR below shows the actual, un-annualized return."
                                    }
                                }
                            },
                            Err(err) => rsx! { p { class: "text-text-tertiary text-sm m-0", "n/a ({err})" } },
                        }
                    }

                    div {
                        p { class: "text-text-secondary text-sm m-0 mb-1",
                            if is_holding_scope { "Holding TWR" } else { "Portfolio TWR (currently-owned holdings with a comp on record)" }
                        }
                        match &perf.twr {
                            Ok(rate) => rsx! { p { class: "data-numeral text-xl m-0", "{percent(*rate)}" } },
                            Err(err) => rsx! { p { class: "text-text-tertiary text-sm m-0", "n/a ({err})" } },
                        }
                    }

                    p { class: "text-text-tertiary text-xs m-0",
                        "TWR isolates how well the investment performed; IRR reflects how well your own buy/sell timing and sizing was. They diverge when capital is added or removed at a good or bad time - neither number is \"wrong\" when that happens."
                    }
                }
            }
        }
    }
}
