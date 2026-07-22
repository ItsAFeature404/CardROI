//! The holding drill-down page: full transaction history, comp history,
//! and realized/unrealized P&L for one holding - the GUI analog of
//! `cardroi holding show` + `cardroi roi --holding-id` + `cardroi comp
//! list` combined into one view. No photo gallery: card photos depend on
//! filesystem-backed storage, and photo capture is explicitly out of
//! scope for a browser tab.
//!
//! What-If (a "simulate selling this" form, not a top-level nav item) and
//! Mark Lost/Damaged both live here rather than on the Buy/Sell/Comp
//! forms: both require an existing holding already in view, unlike Buy
//! which starts fresh.
//!
//! What-If calls `analytics::whatif::holding_whatif` through the web
//! bridge and never writes anything - mirrors `commands::whatif` exactly,
//! including the "give a price or use the latest comp" choice and
//! the HYPOTHETICAL labeling discipline. Mark Lost/Damaged calls
//! `Repository::record_loss` directly, matching `cardroi holding
//! mark-lost`/`mark-damaged`'s flags exactly - no parallel validation
//! logic in either form. `WebBridge::run` has no outer transport-failure
//! layer (no channel to fail), so `bridge.run(...)` returns the plain
//! `CardRoiResult` directly - no double-`Result` unwrapping anywhere in
//! this file.

use cardroi::analytics::roi::{self, HoldingPnl};
use cardroi::analytics::whatif::{self, HypotheticalSale, PriceSource, WhatIfResult};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{
    Appraisal, Holding, HoldingEdit, HoldingStatus, Money, Transaction, TransactionEdit,
    TransactionType,
};
use chrono::{NaiveDate, Utc};
use dioxus::prelude::*;

use crate::components::form_field::FormField;
use crate::web_bridge::WebBridge;

use super::format::{date, money, parse_date, percent};

/// One entry in a holding's ownership timeline - built entirely from data
/// already recorded (a transaction or a comp), never inferred. Comp
/// entries carry the *previous* comp's value so the render layer can
/// show "revised from X" without a second pass over `appraisals`.
#[derive(Clone, Debug, PartialEq)]
enum TimelineEntry {
    Acquisition {
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    Comp {
        date: NaiveDate,
        value: Money,
        source: Option<String>,
        notes: Option<String>,
        revised_from: Option<Money>,
    },
    Adjustment {
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    Sold {
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    /// Covers both Lost and Damaged - the transaction itself doesn't
    /// distinguish them (only `Holding.status` does, checked once at
    /// render time), and a holding can only have one such entry ever,
    /// since it stops being `Owned` the moment this happens.
    LostOrDamaged {
        date: NaiveDate,
        residual_value: Money,
        insurance_recovery: Money,
        cause: Option<String>,
        notes: Option<String>,
    },
}

/// Merges `transactions` and `appraisals` (both already sorted ascending
/// by date - see `Repository::list_transactions_for_holding`/
/// `list_appraisals_for_holding`) into one chronological timeline. No
/// new queries - both lists are already fetched by `load_holding_detail`
/// for other reasons.
fn build_timeline(transactions: &[Transaction], appraisals: &[Appraisal]) -> Vec<TimelineEntry> {
    let mut entries: Vec<(NaiveDate, TimelineEntry)> =
        Vec::with_capacity(transactions.len() + appraisals.len());

    for txn in transactions {
        let entry = match txn.transaction_type {
            TransactionType::Acquisition => TimelineEntry::Acquisition {
                date: txn.transaction_date,
                total: txn.total,
                notes: txn.notes.clone(),
            },
            TransactionType::Adjustment => TimelineEntry::Adjustment {
                date: txn.transaction_date,
                total: txn.total,
                notes: txn.notes.clone(),
            },
            // `residual_value`/`insurance_recovery`/`loss_cause` are only
            // ever populated on the Disposition transaction `record_loss`
            // creates - `record_sale` never sets any of them - so their
            // presence alone distinguishes a loss from a sale, no need to
            // consult `Holding.status` here.
            TransactionType::Disposition
                if txn.residual_value.is_some()
                    || txn.insurance_recovery.is_some()
                    || txn.loss_cause.is_some() =>
            {
                TimelineEntry::LostOrDamaged {
                    date: txn.transaction_date,
                    residual_value: txn.residual_value.unwrap_or(Money::ZERO),
                    insurance_recovery: txn.insurance_recovery.unwrap_or(Money::ZERO),
                    cause: txn.loss_cause.clone(),
                    notes: txn.notes.clone(),
                }
            }
            TransactionType::Disposition => TimelineEntry::Sold {
                date: txn.transaction_date,
                total: txn.total,
                notes: txn.notes.clone(),
            },
        };
        entries.push((txn.transaction_date, entry));
    }

    for (i, appraisal) in appraisals.iter().enumerate() {
        let revised_from = (i > 0).then(|| appraisals[i - 1].appraised_value);
        entries.push((
            appraisal.appraised_date,
            TimelineEntry::Comp {
                date: appraisal.appraised_date,
                value: appraisal.appraised_value,
                source: appraisal.source.clone(),
                notes: appraisal.notes.clone(),
                revised_from,
            },
        ));
    }

    // `sort_by_key` is a stable sort - transactions were pushed before
    // appraisals above, so a same-date transaction (e.g. the acquisition)
    // still renders before a same-date comp, which is the right default
    // (you buy it, then maybe price it, not the other way around).
    entries.sort_by_key(|(date, _)| *date);
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// Days owned so far (still owned) or days the ownership lasted
/// (concluded) - `None` only if `acquired_date` was never set, which
/// shouldn't happen in practice but isn't guaranteed by the type.
fn ownership_duration_days(holding: &Holding, today: NaiveDate) -> Option<i64> {
    let acquired = holding.acquired_date?;
    let end = holding.disposed_date.unwrap_or(today);
    Some((end - acquired).num_days())
}

/// "3 years, 4 months" / "5 months" / "12 days" - deliberately coarse
/// (days become months past 60, months become years past 365) since a
/// collector reads "three years" faster than "1,186 days." The years
/// threshold matters: past a year, "13 months" reads worse than "1
/// year, 1 month" - caught by a test expecting the latter at 400 days.
///
/// `#[allow(dead_code)]`: wired into rendering in the next milestone,
/// only exercised by tests until then.
#[allow(dead_code)]
fn duration_phrase(days: i64) -> String {
    if days < 60 {
        return match days {
            0 => "less than a day".to_string(),
            1 => "1 day".to_string(),
            n => format!("{n} days"),
        };
    }
    if days < 365 {
        return match days / 30 {
            1 => "1 month".to_string(),
            n => format!("{n} months"),
        };
    }
    let years = days / 365;
    let year_part = match years {
        1 => "1 year".to_string(),
        n => format!("{n} years"),
    };
    match (days % 365) / 30 {
        0 => year_part,
        1 => format!("{year_part}, 1 month"),
        n => format!("{year_part}, {n} months"),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HoldingDetailData {
    holding: Holding,
    card_name: String,
    set_name: String,
    status: HoldingStatus,
    pnl: HoldingPnl,
    timeline: Vec<TimelineEntry>,
    ownership_duration_days: Option<i64>,
    // Temporary: the current renderer still reads these two directly.
    // Removed once the merged timeline above replaces the old separate
    // Transaction history / Comp history sections (next milestone).
    transactions: Vec<Transaction>,
    appraisals: Vec<Appraisal>,
}

fn load_holding_detail(holding_id: i64, repo: &Repository) -> CardRoiResult<HoldingDetailData> {
    let holding = repo.get_holding(holding_id)?;
    let card = repo.get_card(holding.card_id)?;
    let set = repo.get_set(card.set_id)?;
    let pnl = roi::holding_pnl(repo, holding_id)?;
    let transactions = repo.list_transactions_for_holding(holding_id)?;
    let appraisals = repo.list_appraisals_for_holding(holding_id)?;
    let timeline = build_timeline(&transactions, &appraisals);
    let ownership_duration_days = ownership_duration_days(&holding, Utc::now().date_naive());

    Ok(HoldingDetailData {
        card_name: card.display_name(),
        set_name: set.name,
        status: holding.status,
        ownership_duration_days,
        holding,
        pnl,
        timeline,
        transactions,
        appraisals,
    })
}

#[component]
pub fn HoldingDetail(id: i64) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut reload_key = use_signal(|| 0u32);

    let data = use_resource(move || {
        let bridge = bridge.clone();
        reload_key();
        async move { bridge.run(move |repo| load_holding_detail(id, repo)).await }
    });

    match &*data.read() {
        None => rsx! {
            div { class: "p-8 text-text-secondary", "Loading..." }
        },
        Some(Err(err)) => rsx! {
            div { class: "p-8 text-loss", "Failed to load holding {id}: {err}" }
        },
        Some(Ok(detail)) => rsx! {
            HoldingDetailBody {
                holding_id: id,
                detail: detail.clone(),
                on_changed: move |_| reload_key.set(reload_key() + 1),
            }
        },
    }
}

#[component]
fn HoldingDetailBody(
    holding_id: i64,
    detail: HoldingDetailData,
    on_changed: EventHandler<()>,
) -> Element {
    let mut editing_holding = use_signal(|| false);
    let mut editing_txn = use_signal(|| None::<i64>);

    rsx! {
        div { class: "p-8 flex flex-col gap-8 max-w-4xl",
            div {
                div { class: "flex justify-between items-start",
                    div {
                        h1 { class: "text-2xl font-semibold m-0", "{detail.card_name}" }
                        p { class: "text-text-secondary text-sm mt-1 mb-0", "{detail.set_name} - {detail.status.as_str()}" }
                    }
                    button {
                        class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                        onclick: move |_| editing_holding.set(!editing_holding()),
                        if editing_holding() { "Cancel" } else { "Edit" }
                    }
                }
                if editing_holding() {
                    HoldingEditForm {
                        key: "{holding_id}",
                        holding_id,
                        holding: detail.holding.clone(),
                        on_saved: move |_| {
                            editing_holding.set(false);
                            on_changed.call(());
                        },
                    }
                }
            }

            PnlSummary { pnl: detail.pnl.clone() }

            div {
                h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Transaction history" }
                if detail.transactions.is_empty() {
                    p { class: "text-text-secondary m-0", "No transactions recorded." }
                } else {
                    div { class: "flex flex-col",
                        for txn in detail.transactions.iter().cloned() {
                            if editing_txn() == Some(txn.id) {
                                TransactionEditForm {
                                    key: "{txn.id}",
                                    txn: txn.clone(),
                                    on_saved: move |_| {
                                        editing_txn.set(None);
                                        on_changed.call(());
                                    },
                                    on_cancel: move |_| editing_txn.set(None),
                                }
                            } else {
                                div { class: "flex justify-between items-center py-3 border-b border-border",
                                    div {
                                        p { class: "m-0", "{txn.transaction_type.as_str()}" }
                                        p { class: "text-text-tertiary text-xs m-0 mt-1", "{date(txn.transaction_date)}" }
                                    }
                                    div { class: "flex items-center gap-3",
                                        p { class: "data-numeral m-0", "{money(txn.total)}" }
                                        button {
                                            class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                                            onclick: move |_| editing_txn.set(Some(txn.id)),
                                            "Edit"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div {
                div { class: "flex justify-between items-center mb-3",
                    h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0", "Comp history" }
                    Link {
                        to: crate::routes::Route::CompForHoldingRoute { holding_id },
                        class: "text-gold text-sm no-underline",
                        "+ Add comp"
                    }
                }
                if detail.appraisals.is_empty() {
                    p { class: "text-text-secondary m-0", "No comps recorded." }
                } else {
                    div { class: "flex flex-col",
                        for comp in detail.appraisals.iter().cloned() {
                            div { class: "flex justify-between items-center py-3 border-b border-border",
                                div {
                                    p { class: "m-0", "{date(comp.appraised_date)}" }
                                    if let Some(source) = &comp.source {
                                        p { class: "text-text-tertiary text-xs m-0 mt-1", "{source}" }
                                    }
                                }
                                p { class: "data-numeral m-0", "{money(comp.appraised_value)}" }
                            }
                        }
                    }
                }
            }

            // Keyed by holding_id: without this key, Dioxus could reuse the
            // same component instance across different holdings when
            // navigating holding-to-holding, leaking one holding's
            // What-If/Mark-Lost form state into another's.
            WhatIfForm { key: "{holding_id}", holding_id }

            if detail.status == HoldingStatus::Owned {
                MarkLostForm { key: "{holding_id}", holding_id, on_changed }
            }

            DeleteHoldingSection {
                key: "{holding_id}",
                holding_id,
                transaction_count: detail.transactions.len(),
            }
        }
    }
}

#[component]
fn PnlSummary(pnl: HoldingPnl) -> Element {
    rsx! {
        div {
            p { class: "text-text-secondary text-sm m-0 mb-1", "Cost basis" }
            p { class: "data-numeral text-2xl m-0", "{money(pnl.cost_basis)}" }

            if let Some(realized) = pnl.realized_pnl {
                p {
                    class: if realized.is_negative() { "data-numeral text-lg mt-1 mb-0 text-loss" } else { "data-numeral text-lg mt-1 mb-0 text-gain" },
                    "realized {money(realized)}"
                    if let Some(pct) = pnl.roi_pct {
                        " ({percent(pct)})"
                    }
                }
            } else if let Some(unrealized) = pnl.unrealized_pnl {
                p {
                    class: if unrealized.is_negative() { "data-numeral text-lg mt-1 mb-0 text-loss" } else { "data-numeral text-lg mt-1 mb-0 text-gain" },
                    "unrealized {money(unrealized)}"
                    if let Some(pct) = pnl.unrealized_roi_pct {
                        " ({percent(pct)})"
                    }
                }
                if let Some(as_of) = pnl.unrealized_pnl_as_of {
                    p { class: "text-text-tertiary text-xs mt-2 mb-0",
                        "As of {as_of} - user-supplied comp, not a live market value"
                    }
                }
            } else {
                p { class: "text-text-secondary text-sm mt-1 mb-0", "No comp on record yet." }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PriceMode {
    Given,
    AtComp,
}

/// The What-If form's raw text inputs, grouped so `load_whatif` takes one
/// argument for "everything the user typed" instead of six.
#[derive(Clone, Debug, Default)]
struct WhatIfInputs {
    price_mode: Option<PriceMode>,
    price: String,
    fees: String,
    shipping: String,
    tax: String,
    other_cost: String,
    date: String,
}

fn load_whatif(
    holding_id: i64,
    inputs: WhatIfInputs,
    repo: &Repository,
) -> CardRoiResult<WhatIfResult> {
    use cardroi::error::CardRoiError;
    use std::str::FromStr;

    let parse_or_zero = |s: &str| -> CardRoiResult<Money> {
        if s.trim().is_empty() {
            Ok(Money::ZERO)
        } else {
            Money::from_str(s)
        }
    };

    let fees = parse_or_zero(&inputs.fees)?;
    let shipping = parse_or_zero(&inputs.shipping)?;
    let tax = parse_or_zero(&inputs.tax)?;
    let other_cost = parse_or_zero(&inputs.other_cost)?;
    let date = if inputs.date.trim().is_empty() {
        chrono::Utc::now().date_naive()
    } else {
        parse_date(&inputs.date).map_err(CardRoiError::validation)?
    };

    let price_mode = inputs.price_mode.unwrap_or(PriceMode::Given);
    let (price, price_source) = match price_mode {
        PriceMode::Given => (Money::from_str(&inputs.price)?, PriceSource::Given),
        PriceMode::AtComp => {
            let comp = repo
                .latest_appraisal_for_holding(holding_id)?
                .ok_or_else(|| {
                    CardRoiError::validation(format!(
                        "holding {holding_id} has no comp on record; enter a price instead"
                    ))
                })?;
            (
                comp.appraised_value,
                PriceSource::LatestAppraisal {
                    appraised_date: comp.appraised_date,
                },
            )
        }
    };

    whatif::holding_whatif(
        repo,
        holding_id,
        HypotheticalSale {
            price,
            fees,
            shipping,
            tax,
            other_cost,
            date,
            price_source,
        },
    )
}

#[component]
fn WhatIfForm(holding_id: i64) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut price_mode = use_signal(|| PriceMode::Given);
    let price_input = use_signal(String::new);
    let fees_input = use_signal(String::new);
    let shipping_input = use_signal(String::new);
    let tax_input = use_signal(String::new);
    let other_cost_input = use_signal(String::new);
    let date_input = use_signal(String::new);
    let mut result = use_signal(|| None::<CardRoiResult<WhatIfResult>>);

    let run = move |_| {
        let bridge = bridge.clone();
        let inputs = WhatIfInputs {
            price_mode: Some(price_mode()),
            price: price_input(),
            fees: fees_input(),
            shipping: shipping_input(),
            tax: tax_input(),
            other_cost: other_cost_input(),
            date: date_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| load_whatif(holding_id, inputs, repo))
                .await;
            result.set(Some(outcome));
        });
    };

    rsx! {
        div {
            h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "What-if: simulate a sale" }
            p { class: "text-text-tertiary text-xs mt-0 mb-3", "Read-only - never writes anything to the database." }

            div { class: "flex gap-2 mb-3",
                button {
                    class: if price_mode() == PriceMode::Given { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| price_mode.set(PriceMode::Given),
                    "Enter a price"
                }
                button {
                    class: if price_mode() == PriceMode::AtComp { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| price_mode.set(PriceMode::AtComp),
                    "Use latest comp"
                }
            }

            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                if price_mode() == PriceMode::Given {
                    FormField { label: "Price", value: price_input, placeholder: "0.00" }
                }
                FormField { label: "Fees", value: fees_input, placeholder: "0.00" }
                FormField { label: "Shipping", value: shipping_input, placeholder: "0.00" }
                FormField { label: "Tax", value: tax_input, placeholder: "0.00" }
                FormField { label: "Other cost", value: other_cost_input, placeholder: "0.00" }
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY (today)" }
            }

            button {
                class: "mt-3 px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer",
                onclick: run,
                "Run"
            }

            match &*result.read() {
                None => rsx! {},
                Some(Err(err)) => rsx! {
                    p { class: "text-loss mt-3", "{err}" }
                },
                Some(Ok(r)) => rsx! {
                    div { class: "mt-4 p-4 bg-surface rounded-radius",
                        p { class: "text-text-tertiary text-xs m-0 mb-2 uppercase tracking-wide", "HYPOTHETICAL - not a real transaction" }
                        p { class: "m-0",
                            "Assumed price {money(r.assumed_price)} ({r.assumed_price_source}) on {date(r.assumed_sale_date)}"
                        }
                        p {
                            class: if r.hypothetical_realized_pnl.is_negative() { "data-numeral text-lg mt-2 mb-0 text-loss" } else { "data-numeral text-lg mt-2 mb-0 text-gain" },
                            "{money(r.hypothetical_realized_pnl)}"
                            if let Some(pct) = r.hypothetical_roi_pct {
                                " ({percent(pct)})"
                            }
                        }
                        if let Some(irr) = r.hypothetical_irr_pct {
                            p { class: "data-numeral text-text-secondary text-sm mt-1 mb-0", "Hypothetical IRR: {percent(irr)}" }
                        }
                    }
                },
            }
        }
    }
}

/// The Mark Lost/Damaged form's raw text inputs, grouped for the same
/// reason as `WhatIfInputs` above.
#[derive(Clone, Debug, Default)]
struct MarkLossInputs {
    date: String,
    residual_value: String,
    insurance_recovery: String,
    cause: String,
    notes: String,
}

fn submit_mark_loss(
    holding_id: i64,
    status: HoldingStatus,
    inputs: MarkLossInputs,
    repo: &Repository,
) -> CardRoiResult<()> {
    use cardroi::error::CardRoiError;
    use std::str::FromStr;

    let date = if inputs.date.trim().is_empty() {
        chrono::Utc::now().date_naive()
    } else {
        parse_date(&inputs.date).map_err(CardRoiError::validation)?
    };
    let parse_or_zero = |s: &str| -> CardRoiResult<Money> {
        if s.trim().is_empty() {
            Ok(Money::ZERO)
        } else {
            Money::from_str(s)
        }
    };
    let residual_value = parse_or_zero(&inputs.residual_value)?;
    let insurance_recovery = parse_or_zero(&inputs.insurance_recovery)?;
    let cause = (!inputs.cause.trim().is_empty()).then_some(inputs.cause);
    let notes = (!inputs.notes.trim().is_empty()).then_some(inputs.notes);

    repo.record_loss(
        holding_id,
        status,
        date,
        residual_value,
        insurance_recovery,
        cause,
        notes,
    )?;
    Ok(())
}

#[component]
fn MarkLostForm(holding_id: i64, on_changed: EventHandler<()>) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut status = use_signal(|| HoldingStatus::Lost);
    let date_input = use_signal(String::new);
    let residual_input = use_signal(String::new);
    let insurance_input = use_signal(String::new);
    let cause_input = use_signal(String::new);
    let notes_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut submitted = use_signal(|| false);

    let submit = move |_| {
        let bridge = bridge.clone();
        let status_value = status();
        let inputs = MarkLossInputs {
            date: date_input(),
            residual_value: residual_input(),
            insurance_recovery: insurance_input(),
            cause: cause_input(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_mark_loss(holding_id, status_value, inputs, repo))
                .await;
            match outcome {
                Ok(()) => {
                    error.set(None);
                    submitted.set(true);
                    on_changed.call(());
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    if submitted() {
        return rsx! {
            div {
                h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Mark Lost / Damaged" }
                p { class: "text-gain m-0", "Recorded." }
            }
        };
    }

    rsx! {
        div {
            h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Mark Lost / Damaged" }

            div { class: "flex gap-2 mb-3",
                button {
                    class: if status() == HoldingStatus::Lost { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| status.set(HoldingStatus::Lost),
                    "Lost"
                }
                button {
                    class: if status() == HoldingStatus::Damaged { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| status.set(HoldingStatus::Damaged),
                    "Damaged"
                }
            }

            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY (today)" }
                FormField { label: "Residual value", value: residual_input, placeholder: "0.00" }
                FormField { label: "Insurance recovery", value: insurance_input, placeholder: "0.00" }
                FormField { label: "Cause", value: cause_input, placeholder: "e.g. water damage" }
                FormField { label: "Notes", value: notes_input, placeholder: "" }
            }

            button {
                class: "mt-3 px-4 py-2 rounded-radius bg-loss text-canvas border-none font-semibold cursor-pointer",
                onclick: submit,
                "Record"
            }

            if let Some(err) = error() {
                p { class: "text-loss mt-3", "{err}" }
            }
        }
    }
}

/// The holding-attribute edit form's raw text inputs, grouped for the
/// same reason as `WhatIfInputs`/`MarkLossInputs` above.
#[derive(Clone, Debug, Default)]
struct HoldingEditInputs {
    serial_number: String,
    grade: String,
    grading_company: String,
    cert_number: String,
    notes: String,
}

fn submit_holding_edit(
    holding_id: i64,
    inputs: HoldingEditInputs,
    repo: &Repository,
) -> CardRoiResult<Holding> {
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    let edit = HoldingEdit {
        serial_number: non_empty(inputs.serial_number),
        grade: non_empty(inputs.grade),
        grading_company: non_empty(inputs.grading_company),
        cert_number: non_empty(inputs.cert_number),
        notes: non_empty(inputs.notes),
    };
    repo.update_holding(holding_id, &edit)
}

/// Corrects a holding's own physical/grading attributes - not its status
/// or which card it is (those are governed by `MarkLostForm`/the Sell
/// form, not this one). Reached via the "Edit" toggle next to the card
/// title.
#[component]
fn HoldingEditForm(holding_id: i64, holding: Holding, on_saved: EventHandler<()>) -> Element {
    let bridge = use_context::<WebBridge>();
    let serial_input = use_signal(|| holding.serial_number.clone().unwrap_or_default());
    let grade_input = use_signal(|| holding.grade.clone().unwrap_or_default());
    let grading_company_input = use_signal(|| holding.grading_company.clone().unwrap_or_default());
    let cert_input = use_signal(|| holding.cert_number.clone().unwrap_or_default());
    let notes_input = use_signal(|| holding.notes.clone().unwrap_or_default());
    let mut error = use_signal(|| None::<String>);

    let submit = move |_| {
        let bridge = bridge.clone();
        let inputs = HoldingEditInputs {
            serial_number: serial_input(),
            grade: grade_input(),
            grading_company: grading_company_input(),
            cert_number: cert_input(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_holding_edit(holding_id, inputs, repo))
                .await;
            match outcome {
                Ok(_) => {
                    error.set(None);
                    on_saved.call(());
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    rsx! {
        div { class: "mt-3 p-4 bg-surface rounded-radius flex flex-col gap-3",
            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                FormField { label: "Serial", value: serial_input, placeholder: "e.g. 12/25" }
                FormField { label: "Grade", value: grade_input, placeholder: "e.g. 10" }
                FormField { label: "Grading company", value: grading_company_input, placeholder: "e.g. PSA" }
                FormField { label: "Cert number", value: cert_input, placeholder: "" }
                FormField { label: "Notes", value: notes_input, placeholder: "" }
            }
            button {
                class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer self-start",
                onclick: submit,
                "Save"
            }
            if let Some(err) = error() {
                p { class: "text-loss m-0", "{err}" }
            }
        }
    }
}

/// The transaction edit form's raw text inputs, grouped for the same
/// reason as the other forms' input-groupings above.
#[derive(Clone, Debug, Default)]
struct TransactionEditInputs {
    date: String,
    price: String,
    fees: String,
    shipping: String,
    tax: String,
    other_cost: String,
    counterparty: String,
    platform: String,
    external_ref: String,
    notes: String,
}

fn submit_transaction_edit(
    txn_id: i64,
    currency: String,
    inputs: TransactionEditInputs,
    repo: &Repository,
) -> CardRoiResult<Transaction> {
    use cardroi::error::CardRoiError;
    use std::str::FromStr;

    let transaction_date = parse_date(&inputs.date).map_err(CardRoiError::validation)?;
    let price = Money::from_str(&inputs.price)?;
    let parse_or_zero = |s: &str| -> CardRoiResult<Money> {
        if s.trim().is_empty() {
            Ok(Money::ZERO)
        } else {
            Money::from_str(s)
        }
    };
    let fees = parse_or_zero(&inputs.fees)?;
    let shipping = parse_or_zero(&inputs.shipping)?;
    let tax = parse_or_zero(&inputs.tax)?;
    let other_cost = parse_or_zero(&inputs.other_cost)?;
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);

    let edit = TransactionEdit {
        transaction_date,
        price,
        fees,
        shipping,
        tax,
        other_cost,
        currency,
        counterparty: non_empty(inputs.counterparty),
        platform: non_empty(inputs.platform),
        external_ref: non_empty(inputs.external_ref),
        notes: non_empty(inputs.notes),
    };
    repo.update_transaction(txn_id, &edit)
}

/// Corrects an existing transaction's own fields (wrong price, wrong
/// date, a typo) - not its type or which holding it's on. Reached via the
/// "Edit" link on that transaction's row in Transaction history.
#[component]
fn TransactionEditForm(
    txn: Transaction,
    on_saved: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let bridge = use_context::<WebBridge>();
    let date_input = use_signal(|| date(txn.transaction_date));
    let price_input = use_signal(|| txn.price.to_string());
    let fees_input = use_signal(|| txn.fees.to_string());
    let shipping_input = use_signal(|| txn.shipping.to_string());
    let tax_input = use_signal(|| txn.tax.to_string());
    let other_cost_input = use_signal(|| txn.other_cost.to_string());
    let counterparty_input = use_signal(|| txn.counterparty.clone().unwrap_or_default());
    let platform_input = use_signal(|| txn.platform.clone().unwrap_or_default());
    let external_ref_input = use_signal(|| txn.external_ref.clone().unwrap_or_default());
    let notes_input = use_signal(|| txn.notes.clone().unwrap_or_default());
    let mut error = use_signal(|| None::<String>);

    let txn_id = txn.id;
    let currency = txn.currency.clone();

    let submit = move |_| {
        let bridge = bridge.clone();
        let currency = currency.clone();
        let inputs = TransactionEditInputs {
            date: date_input(),
            price: price_input(),
            fees: fees_input(),
            shipping: shipping_input(),
            tax: tax_input(),
            other_cost: other_cost_input(),
            counterparty: counterparty_input(),
            platform: platform_input(),
            external_ref: external_ref_input(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_transaction_edit(txn_id, currency, inputs, repo))
                .await;
            match outcome {
                Ok(_) => {
                    error.set(None);
                    on_saved.call(());
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    rsx! {
        div { class: "my-3 p-4 bg-surface rounded-radius flex flex-col gap-3",
            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY" }
                FormField { label: "Price", value: price_input, placeholder: "0.00" }
                FormField { label: "Fees", value: fees_input, placeholder: "0.00" }
                FormField { label: "Shipping", value: shipping_input, placeholder: "0.00" }
                FormField { label: "Tax", value: tax_input, placeholder: "0.00" }
                FormField { label: "Other cost", value: other_cost_input, placeholder: "0.00" }
                FormField { label: "Counterparty", value: counterparty_input, placeholder: "" }
                FormField { label: "Platform", value: platform_input, placeholder: "" }
                FormField { label: "Reference", value: external_ref_input, placeholder: "" }
                FormField { label: "Notes", value: notes_input, placeholder: "" }
            }
            div { class: "flex gap-2",
                button {
                    class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer",
                    onclick: submit,
                    "Save"
                }
                button {
                    class: "px-4 py-2 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer",
                    onclick: move |_| on_cancel.call(()),
                    "Cancel"
                }
            }
            if let Some(err) = error() {
                p { class: "text-loss m-0", "{err}" }
            }
        }
    }
}

fn delete_holding_cascade(holding_id: i64, repo: &Repository) -> CardRoiResult<()> {
    repo.delete_holding_cascade(holding_id)
}

/// A permanent, explicit "delete this holding and its whole transaction
/// history" action - `transactions.holding_id` normally restricts a plain
/// delete once any transaction exists (see `Repository::
/// delete_holding_cascade`'s doc comment), so this is the one place that
/// deliberately bypasses it. Two-stage confirm (a plain "Delete holding"
/// button reveals a named warning + a differently-worded final button)
/// rather than a single click, since unlike everything else in this app,
/// this is real, permanent data loss.
#[component]
fn DeleteHoldingSection(holding_id: i64, transaction_count: usize) -> Element {
    let bridge = use_context::<WebBridge>();
    let navigator = use_navigator();
    let mut confirming = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let confirm_delete = move |_| {
        let bridge = bridge.clone();
        spawn(async move {
            let outcome = bridge
                .run(move |repo| delete_holding_cascade(holding_id, repo))
                .await;
            match outcome {
                Ok(()) => {
                    navigator.push(crate::routes::Route::PortfolioRoute {});
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    rsx! {
        div { class: "mt-4 pt-6 border-t border-border",
            h2 { class: "text-sm font-semibold text-loss uppercase tracking-wide m-0 mb-3", "Danger zone" }
            if confirming() {
                div { class: "p-4 bg-surface rounded-radius flex flex-col gap-3",
                    p { class: "text-loss m-0 font-semibold",
                        "This permanently deletes this holding and its {transaction_count} transaction(s). This cannot be undone."
                    }
                    div { class: "flex gap-2",
                        button {
                            class: "px-4 py-2 rounded-radius bg-loss text-canvas border-none font-semibold cursor-pointer",
                            onclick: confirm_delete,
                            "Yes, delete permanently"
                        }
                        button {
                            class: "px-4 py-2 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer",
                            onclick: move |_| confirming.set(false),
                            "Cancel"
                        }
                    }
                    if let Some(err) = error() {
                        p { class: "text-loss m-0", "{err}" }
                    }
                }
            } else {
                button {
                    class: "px-4 py-2 rounded-radius bg-transparent text-loss border border-loss cursor-pointer",
                    onclick: move |_| confirming.set(true),
                    "Delete holding"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use cardroi::db::open_in_memory;
    use cardroi::models::{NewCard, NewHolding, NewSet, NewTransaction};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    // This crate only ever builds for wasm32 (see Cargo.toml's dependency
    // tables), so its tests need a wasm-aware harness - plain #[test]
    // compiles but can never execute here (confirmed directly: the linked
    // wasm-bindgen glue needs a JS host, which no bare wasm32 runtime
    // provides). No `wasm_bindgen_test_configure!` call needed - Node is
    // the default target, and none of these tests touch the DOM, only
    // the in-memory repository, so there's no reason to force a browser.

    fn repo_with_owned_holding() -> (Repository, i64) {
        let repo = Repository::new(open_in_memory().unwrap());
        let set = repo
            .create_set(&NewSet {
                name: "Test Set".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        let card = repo
            .create_card(&NewCard {
                set_id: set.id,
                card_number: "1".to_string(),
                player_name: "Test Player".to_string(),
                ..Default::default()
            })
            .unwrap();
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: card.id,
                    ..Default::default()
                },
                NewTransaction {
                    price: Money::from_str("500.00").unwrap(),
                    ..Default::default()
                },
            )
            .unwrap();
        (repo, holding.id)
    }

    // Mirrors tests/cli_whatif.rs's whatif_never_writes_anything_to_the_database
    // - the same "run it twice, confirm nothing changed" pattern, exercised
    // against this web-specific wrapper rather than assuming the library
    // function it calls is enough proof this code path is also write-free.
    #[wasm_bindgen_test]
    fn running_whatif_twice_never_changes_the_holdings_real_state() {
        let (repo, holding_id) = repo_with_owned_holding();

        load_whatif(
            holding_id,
            WhatIfInputs {
                price: "800.00".to_string(),
                ..Default::default()
            },
            &repo,
        )
        .unwrap();
        load_whatif(
            holding_id,
            WhatIfInputs {
                price: "1200.00".to_string(),
                ..Default::default()
            },
            &repo,
        )
        .unwrap();

        let holding = repo.get_holding(holding_id).unwrap();
        assert_eq!(holding.status, HoldingStatus::Owned);
        let transactions = repo.list_transactions_for_holding(holding_id).unwrap();
        assert_eq!(
            transactions.len(),
            1,
            "only the original acquisition should exist"
        );
    }

    #[wasm_bindgen_test]
    fn mark_loss_flips_status_and_matches_repository_record_loss_directly() {
        let (repo, holding_id) = repo_with_owned_holding();

        submit_mark_loss(
            holding_id,
            HoldingStatus::Damaged,
            MarkLossInputs {
                date: "06-01-2026".to_string(),
                residual_value: "50.00".to_string(),
                insurance_recovery: "100.00".to_string(),
                cause: "water damage".to_string(),
                notes: String::new(),
            },
            &repo,
        )
        .unwrap();

        let holding = repo.get_holding(holding_id).unwrap();
        assert_eq!(holding.status, HoldingStatus::Damaged);
        let pnl = roi::holding_pnl(&repo, holding_id).unwrap();
        // proceeds 150.00 (50 residual + 100 insurance) - 500.00 cost basis
        assert_eq!(pnl.realized_pnl, Some(Money::from_str("-350.00").unwrap()));
    }

    fn test_date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn acquisition_txn(date: NaiveDate, total: &str, notes: Option<&str>) -> Transaction {
        Transaction {
            id: 1,
            holding_id: 1,
            transaction_type: TransactionType::Acquisition,
            transaction_date: date,
            price: Money::from_str(total).unwrap(),
            fees: Money::ZERO,
            shipping: Money::ZERO,
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            total: Money::from_str(total).unwrap(),
            currency: "USD".to_string(),
            counterparty: None,
            platform: None,
            external_ref: None,
            notes: notes.map(str::to_string),
            residual_value: None,
            insurance_recovery: None,
            loss_cause: None,
            created_at: Utc::now(),
        }
    }

    fn sale_txn(date: NaiveDate, total: &str) -> Transaction {
        Transaction {
            transaction_type: TransactionType::Disposition,
            ..acquisition_txn(date, total, None)
        }
    }

    fn loss_txn(date: NaiveDate, residual: &str, cause: &str) -> Transaction {
        Transaction {
            transaction_type: TransactionType::Disposition,
            residual_value: Some(Money::from_str(residual).unwrap()),
            insurance_recovery: Some(Money::ZERO),
            loss_cause: Some(cause.to_string()),
            ..acquisition_txn(date, "0.00", None)
        }
    }

    fn comp(date: NaiveDate, value: &str) -> Appraisal {
        Appraisal {
            id: 1,
            holding_id: 1,
            appraised_value: Money::from_str(value).unwrap(),
            appraised_date: date,
            source: None,
            notes: None,
            created_at: Utc::now(),
        }
    }

    #[wasm_bindgen_test]
    fn build_timeline_interleaves_transactions_and_comps_chronologically() {
        let transactions = vec![acquisition_txn(
            test_date(2026, 1, 1),
            "500.00",
            Some("found at a show"),
        )];
        let appraisals = vec![
            comp(test_date(2026, 3, 1), "600.00"),
            comp(test_date(2026, 6, 1), "900.00"),
        ];

        let timeline = build_timeline(&transactions, &appraisals);

        assert_eq!(timeline.len(), 3);
        assert!(
            matches!(&timeline[0], TimelineEntry::Acquisition { notes, .. } if notes.as_deref() == Some("found at a show"))
        );
        match &timeline[1] {
            TimelineEntry::Comp {
                value,
                revised_from,
                ..
            } => {
                assert_eq!(*value, Money::from_str("600.00").unwrap());
                assert_eq!(*revised_from, None, "the first comp revises nothing");
            }
            other => panic!("expected the first comp, got {other:?}"),
        }
        match &timeline[2] {
            TimelineEntry::Comp {
                value,
                revised_from,
                ..
            } => {
                assert_eq!(*value, Money::from_str("900.00").unwrap());
                assert_eq!(*revised_from, Some(Money::from_str("600.00").unwrap()));
            }
            other => panic!("expected the second comp, got {other:?}"),
        }
    }

    #[wasm_bindgen_test]
    fn build_timeline_distinguishes_a_sale_from_a_loss_by_transaction_fields_alone() {
        let sold = vec![sale_txn(test_date(2026, 6, 1), "800.00")];
        let timeline = build_timeline(&sold, &[]);
        assert!(matches!(timeline[0], TimelineEntry::Sold { .. }));

        let lost = vec![loss_txn(test_date(2026, 6, 1), "50.00", "water damage")];
        let timeline = build_timeline(&lost, &[]);
        match &timeline[0] {
            TimelineEntry::LostOrDamaged {
                residual_value,
                cause,
                ..
            } => {
                assert_eq!(*residual_value, Money::from_str("50.00").unwrap());
                assert_eq!(cause.as_deref(), Some("water damage"));
            }
            other => panic!("expected a loss entry, got {other:?}"),
        }
    }

    #[wasm_bindgen_test]
    fn ownership_duration_uses_disposed_date_when_concluded_else_today() {
        let mut holding = Holding {
            id: 1,
            card_id: 1,
            serial_number: None,
            grade: None,
            grading_company: None,
            cert_number: None,
            status: HoldingStatus::Owned,
            acquired_date: Some(test_date(2026, 1, 1)),
            disposed_date: None,
            notes: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Still owned: measured against "today."
        assert_eq!(
            ownership_duration_days(&holding, test_date(2026, 1, 11)),
            Some(10)
        );

        // Concluded: measured against disposed_date, not today, even if
        // "today" is much later.
        holding.status = HoldingStatus::Sold;
        holding.disposed_date = Some(test_date(2026, 1, 6));
        assert_eq!(
            ownership_duration_days(&holding, test_date(2026, 12, 31)),
            Some(5)
        );
    }

    #[wasm_bindgen_test]
    fn duration_phrase_is_coarse_and_reads_naturally() {
        assert_eq!(duration_phrase(0), "less than a day");
        assert_eq!(duration_phrase(1), "1 day");
        assert_eq!(duration_phrase(4), "4 days");
        assert_eq!(duration_phrase(90), "3 months");
        assert_eq!(duration_phrase(400), "1 year, 1 month");
        assert_eq!(duration_phrase(800), "2 years, 2 months");
        assert_eq!(duration_phrase(730), "2 years");
    }
}
