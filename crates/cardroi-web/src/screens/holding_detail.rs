//! The holding drill-down page: full transaction history, comp history,
//! and realized/unrealized P&L for one holding - the GUI analog of
//! `cardroi holding show` + `cardroi roi --holding-id` + `cardroi comp
//! list` combined into one view. One photo per holding (not a gallery -
//! the repository supports many, this page shows/replaces one), uploaded
//! via `PhotoStorage::Inline` since this crate has no filesystem to write
//! a disk-backed photo to (see `components::photo::PhotoCapture`).
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

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cardroi::analytics::roi::{self, HoldingPnl};
use cardroi::analytics::whatif::{self, HypotheticalSale, PriceSource, WhatIfResult};
use cardroi::db::repository::{PhotoStorage, Repository};
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{
    Appraisal, Card, CardEdit, Holding, HoldingEdit, HoldingImage, HoldingStatus, Money,
    Transaction, TransactionEdit, TransactionType,
};
use chrono::{NaiveDate, Utc};
use dioxus::prelude::*;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::ld_icons::{LdImage, LdPencil, LdX};

use crate::components::form_field::FormField;
use crate::components::photo::PhotoCapture;
use crate::web_bridge::WebBridge;

use super::format::{date, duration_phrase, money, parse_date, parse_optional_i32, percent};

/// One entry in a holding's ownership timeline - built entirely from data
/// already recorded (a transaction or a comp), never inferred. Comp
/// entries carry the *previous* comp's value so the render layer can
/// show "revised from X" without a second pass over `appraisals`.
#[derive(Clone, Debug, PartialEq)]
enum TimelineEntry {
    Acquisition {
        id: i64,
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    /// `id` is only used as a stable render key here - comps have no
    /// edit affordance anywhere in this app today (only adding one is
    /// supported), not this brief's job to add.
    Comp {
        id: i64,
        date: NaiveDate,
        value: Money,
        source: Option<String>,
        notes: Option<String>,
        revised_from: Option<Money>,
    },
    Adjustment {
        id: i64,
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    Sold {
        id: i64,
        date: NaiveDate,
        total: Money,
        notes: Option<String>,
    },
    /// Covers both Lost and Damaged - the transaction itself doesn't
    /// distinguish them (only `Holding.status` does, checked once at
    /// render time), and a holding can only have one such entry ever,
    /// since it stops being `Owned` the moment this happens.
    LostOrDamaged {
        id: i64,
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
                id: txn.id,
                date: txn.transaction_date,
                total: txn.total,
                notes: txn.notes.clone(),
            },
            TransactionType::Adjustment => TimelineEntry::Adjustment {
                id: txn.id,
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
                    id: txn.id,
                    date: txn.transaction_date,
                    residual_value: txn.residual_value.unwrap_or(Money::ZERO),
                    insurance_recovery: txn.insurance_recovery.unwrap_or(Money::ZERO),
                    cause: txn.loss_cause.clone(),
                    notes: txn.notes.clone(),
                }
            }
            TransactionType::Disposition => TimelineEntry::Sold {
                id: txn.id,
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
                id: appraisal.id,
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

/// A stable, unique key for rendering a mixed-variant list of timeline
/// entries - `TimelineEntry` itself has no single common `id` field
/// across variants (comps and transactions are different tables).
fn timeline_entry_key(entry: &TimelineEntry) -> String {
    match entry {
        TimelineEntry::Acquisition { id, .. }
        | TimelineEntry::Adjustment { id, .. }
        | TimelineEntry::Sold { id, .. }
        | TimelineEntry::LostOrDamaged { id, .. } => format!("txn-{id}"),
        TimelineEntry::Comp { id, .. } => format!("comp-{id}"),
    }
}

/// The backing `Transaction.id` for an editable entry, or `None` for a
/// `Comp` (no edit form exists for one anywhere in this app).
fn timeline_entry_txn_id(entry: &TimelineEntry) -> Option<i64> {
    match entry {
        TimelineEntry::Acquisition { id, .. }
        | TimelineEntry::Adjustment { id, .. }
        | TimelineEntry::Sold { id, .. }
        | TimelineEntry::LostOrDamaged { id, .. } => Some(*id),
        TimelineEntry::Comp { .. } => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HoldingDetailData {
    holding: Holding,
    card: Card,
    card_name: String,
    set_name: String,
    status: HoldingStatus,
    pnl: HoldingPnl,
    timeline: Vec<TimelineEntry>,
    ownership_duration_days: Option<i64>,
    // Kept (not folded into `timeline`) because `TransactionEditForm`
    // needs the *full* `Transaction` (fees, shipping, counterparty, ...)
    // to edit one, and `DeleteHoldingSection` needs the count - the
    // timeline only carries the narrower fields the read view shows.
    // `appraisals` has no equivalent need (comps have no edit form
    // anywhere in this app) and isn't kept.
    transactions: Vec<Transaction>,
    /// The primary photo, if one's been added - `list_photos_for_holding`
    /// already orders primary-first, so `.next()` is enough; no second
    /// query or sort needed.
    primary_photo: Option<HoldingImage>,
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
    let primary_photo = repo.list_photos_for_holding(holding_id)?.into_iter().next();

    Ok(HoldingDetailData {
        card_name: card.display_name(),
        set_name: set.name,
        status: holding.status,
        ownership_duration_days,
        holding,
        card,
        pnl,
        timeline,
        transactions,
        primary_photo,
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
    // Experimental (Card Details presentation prototype): one page-level
    // edit toggle replaces the two separate, always-visible "Edit"
    // affordances that used to sit here - no edit control shows anywhere
    // on this page until this is switched on.
    let bridge = use_context::<WebBridge>();
    let mut edit_mode = use_signal(|| false);
    let mut editing_holding = use_signal(|| false);
    let mut editing_card = use_signal(|| false);
    let mut editing_txn = use_signal(|| None::<i64>);

    let notes = detail.holding.notes.as_deref().unwrap_or("").trim();

    rsx! {
        div { class: "p-8 flex flex-col gap-12 max-w-4xl animate-settle-in",
            // The card's own identity, presented as the object itself -
            // not a header on a record. `font-brand` is this app's one
            // typeface reserved for exactly this kind of singular,
            // non-tabular moment; nothing else on the page shares it.
            div { class: "relative rounded-[28px] bg-surface p-10",
                button {
                    class: "absolute top-6 right-6 w-8 h-8 flex items-center justify-center rounded-full bg-transparent border-none cursor-pointer text-text-tertiary hover:text-gold hover:bg-surface-elevated transition-colors duration-[var(--duration-standard)] ease-standard",
                    "aria-label": if edit_mode() { "Done editing" } else { "Edit this holding" },
                    onclick: move |_| {
                        let next = !edit_mode();
                        edit_mode.set(next);
                        if !next {
                            editing_holding.set(false);
                            editing_card.set(false);
                            editing_txn.set(None);
                        }
                    },
                    Icon { icon: LdPencil, width: 16, height: 16 }
                }
                div { class: "flex gap-4 items-start",
                    // A fixed-size slot regardless of whether a photo
                    // exists yet, so adding one later never reshuffles
                    // the hero's geometry. `group` + opacity-0/group-
                    // hover:opacity-100 on the delete button below - it's
                    // only ever discoverable by hovering the photo
                    // itself, not a separate always-visible control.
                    div { class: "group relative w-32 h-32 shrink-0 rounded-2xl overflow-hidden bg-surface-elevated flex items-center justify-center",
                        if let Some(photo) = &detail.primary_photo {
                            img {
                                class: "w-full h-full object-cover",
                                src: "data:image/jpeg;base64,{BASE64.encode(&photo.thumbnail_data)}",
                                alt: "{detail.card_name}",
                            }
                            button {
                                class: "absolute top-1 right-1 w-6 h-6 flex items-center justify-center rounded-full bg-canvas/70 text-text-tertiary opacity-0 group-hover:opacity-100 border-none cursor-pointer transition-opacity duration-[var(--duration-standard)] ease-standard hover:text-loss",
                                "aria-label": "Remove photo",
                                onclick: {
                                    let bridge = bridge.clone();
                                    let photo_id = photo.id;
                                    move |_| {
                                        let bridge = bridge.clone();
                                        spawn(async move {
                                            let _ = bridge
                                                .run(move |repo| repo.delete_photo(photo_id, PhotoStorage::Inline))
                                                .await;
                                            on_changed.call(());
                                        });
                                    }
                                },
                                Icon { icon: LdX, width: 14, height: 14 }
                            }
                        } else {
                            Icon { icon: LdImage, width: 24, height: 24, class: "text-text-tertiary opacity-50" }
                        }
                    }
                    div { class: "flex-1 min-w-0",
                        h1 { class: "font-brand text-4xl m-0 pr-10", "{detail.card_name}" }
                        p { class: "text-text-secondary text-sm mt-2 mb-0", "{detail.set_name}" }
                        div { class: "flex flex-wrap gap-3 mt-4",
                            if let Some(serial) = &detail.holding.serial_number {
                                span { class: "px-2.5 py-1 rounded-[10px] bg-surface-elevated text-text-secondary text-xs font-data", "#{serial}" }
                            }
                            if let Some(grade) = &detail.holding.grade {
                                span { class: "px-2.5 py-1 rounded-[10px] bg-gold text-canvas text-xs font-semibold",
                                    if let Some(company) = &detail.holding.grading_company {
                                        "{company} {grade}"
                                    } else {
                                        "{grade}"
                                    }
                                }
                            }
                        }
                        if let Some(days) = detail.ownership_duration_days {
                            p { class: "text-text-tertiary text-sm mt-5 mb-0",
                                if detail.status == HoldingStatus::Owned {
                                    "Yours for {duration_phrase(days)}"
                                } else {
                                    "Owned for {duration_phrase(days)}"
                                }
                            }
                        }
                    }
                }
                if edit_mode() {
                    div { class: "mt-6 flex flex-col gap-3 items-start",
                        button {
                            class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                            onclick: move |_| editing_holding.set(!editing_holding()),
                            if editing_holding() { "Cancel" } else { "Edit details" }
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
                        button {
                            class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                            onclick: move |_| editing_card.set(!editing_card()),
                            if editing_card() { "Cancel" } else { "Edit card details" }
                        }
                        if editing_card() {
                            CardEditForm {
                                key: "{detail.card.id}",
                                card: detail.card.clone(),
                                on_saved: move |_| {
                                    editing_card.set(false);
                                    on_changed.call(());
                                },
                            }
                        }
                        PhotoCapture {
                            key: "{holding_id}",
                            holding_id,
                            current_photo_id: detail.primary_photo.as_ref().map(|p| p.id),
                            on_uploaded: move |_| on_changed.call(()),
                        }
                    }
                }
            }

            PnlSummary { pnl: detail.pnl.clone() }

            if !notes.is_empty() {
                p { class: "font-brand text-lg text-text-secondary italic m-0 whitespace-pre-wrap", "\u{201c}{notes}\u{201d}" }
            }

            div {
                div { class: "flex justify-between items-center mb-6",
                    h2 { class: "text-xs font-medium text-text-tertiary uppercase tracking-wide m-0", "Timeline" }
                    Link {
                        to: crate::routes::Route::CompForHoldingRoute { holding_id },
                        class: "text-gold text-sm no-underline",
                        "+ Add comp"
                    }
                }
                if detail.timeline.is_empty() {
                    p { class: "text-text-secondary m-0", "Nothing recorded yet." }
                } else {
                    // A connected thread down the left edge, not a bordered
                    // list - this is the ownership's own line running
                    // through it, not a table of rows.
                    div { class: "flex flex-col border-l-2 border-border ml-1",
                        for entry in detail.timeline.iter().cloned() {
                            {
                                let editable_id = timeline_entry_txn_id(&entry);
                                if edit_mode() && editable_id.is_some() && editing_txn() == editable_id {
                                    let txn_id = editable_id.expect("checked Some above");
                                    let txn = detail
                                        .transactions
                                        .iter()
                                        .find(|t| t.id == txn_id)
                                        .cloned()
                                        .expect("every editable timeline entry has a backing transaction");
                                    rsx! {
                                        div { class: "pl-8 pb-8",
                                            TransactionEditForm {
                                                key: "{timeline_entry_key(&entry)}",
                                                txn,
                                                on_saved: move |_| {
                                                    editing_txn.set(None);
                                                    on_changed.call(());
                                                },
                                                on_cancel: move |_| editing_txn.set(None),
                                            }
                                        }
                                    }
                                } else {
                                    rsx! {
                                        TimelineEntryView {
                                            key: "{timeline_entry_key(&entry)}",
                                            entry,
                                            holding_status: detail.status,
                                            edit_mode: edit_mode(),
                                            on_edit: move |id| editing_txn.set(Some(id)),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Keyed by holding_id: without this key, Dioxus could reuse the
            // same component instance across different holdings when
            // navigating holding-to-holding, leaking one holding's
            // What-If/Mark-Lost form state into another's.
            //
            // What-If and the actual means to act on it sit together
            // deliberately - the natural next thought after reflecting on
            // this ownership is "should I sell," and the answer shouldn't
            // require leaving the page to act on.
            div { class: "flex flex-col gap-4 items-start",
                WhatIfForm { key: "{holding_id}", holding_id }
                if detail.status == HoldingStatus::Owned {
                    Link {
                        to: crate::routes::Route::SellForHoldingRoute { holding_id },
                        class: "text-gold text-sm no-underline",
                        "Ready? Sell this holding \u{2192}"
                    }
                }
            }

            // Set apart with the same "distinct, weightier zone" treatment
            // as the danger zone below - a real turn in this ownership's
            // story, not a decision made in the same register as What-If.
            if detail.status == HoldingStatus::Owned {
                div { class: "pt-6 border-t border-border",
                    MarkLostForm { key: "{holding_id}", holding_id, on_changed }
                }
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

/// Dispatches one merged-timeline entry to its read-only rendering. Four
/// of the five variants share an identical label/date/amount/notes/edit
/// shape (`TimelineRow`, below) - `Comp` differs enough (a source, a
/// revised-from comparison, no edit affordance, quieter emphasis - a
/// comp is a data point, not an event the way a buy or sell is) to
/// build its own amount string instead.
///
/// Experimental (Card Details presentation prototype): entries no
/// longer uniformly right-align a dollar figure like a ledger row - a
/// comp's amount reads quieter than an acquisition's or a sale's, since
/// the number isn't equally the point in both cases.
#[component]
fn TimelineEntryView(
    entry: TimelineEntry,
    holding_status: HoldingStatus,
    edit_mode: bool,
    on_edit: EventHandler<i64>,
) -> Element {
    match entry {
        TimelineEntry::Acquisition {
            id,
            date: entry_date,
            total,
            notes,
        } => rsx! {
            TimelineRow { label: "Bought".to_string(), entry_date, amount: money(total), notes, edit_id: Some(id), edit_mode, on_edit, quiet: false }
        },
        TimelineEntry::Adjustment {
            id,
            date: entry_date,
            total,
            notes,
        } => rsx! {
            TimelineRow { label: "Cost basis adjustment".to_string(), entry_date, amount: money(total), notes, edit_id: Some(id), edit_mode, on_edit, quiet: false }
        },
        TimelineEntry::Sold {
            id,
            date: entry_date,
            total,
            notes,
        } => rsx! {
            TimelineRow { label: "Sold".to_string(), entry_date, amount: money(total), notes, edit_id: Some(id), edit_mode, on_edit, quiet: false }
        },
        TimelineEntry::LostOrDamaged {
            id,
            date: entry_date,
            residual_value,
            insurance_recovery,
            cause,
            notes,
        } => {
            let label = if holding_status == HoldingStatus::Damaged {
                "Damaged"
            } else {
                "Lost"
            };
            let proceeds = residual_value + insurance_recovery;
            let combined_notes = match (cause, notes) {
                (Some(cause), Some(n)) => Some(format!("Cause: {cause}. {n}")),
                (Some(cause), None) => Some(format!("Cause: {cause}")),
                (None, n) => n,
            };
            rsx! {
                TimelineRow { label: label.to_string(), entry_date, amount: money(proceeds), notes: combined_notes, edit_id: Some(id), edit_mode, on_edit, quiet: false }
            }
        }
        TimelineEntry::Comp {
            date: entry_date,
            value,
            source,
            notes,
            revised_from,
            ..
        } => {
            let label = source.unwrap_or_else(|| "Comp".to_string());
            let amount = match revised_from {
                Some(prev) => format!("{} (from {})", money(value), money(prev)),
                None => money(value),
            };
            rsx! {
                TimelineRow { label, entry_date, amount, notes, edit_id: None, edit_mode, on_edit, quiet: true }
            }
        }
    }
}

/// Experimental (Card Details presentation prototype): a shared node on
/// the ownership's own connecting thread (see the parent's `border-l-2`)
/// rather than a bordered table row - a small marker instead of a
/// horizontal rule, generous space instead of a divider. Edit disappears
/// entirely unless the page is in `edit_mode`.
#[component]
fn TimelineRow(
    label: String,
    entry_date: NaiveDate,
    amount: String,
    notes: Option<String>,
    edit_id: Option<i64>,
    edit_mode: bool,
    on_edit: EventHandler<i64>,
    quiet: bool,
) -> Element {
    let amount_class = if quiet {
        "data-numeral text-sm text-text-tertiary m-0"
    } else {
        "data-numeral text-lg m-0"
    };
    rsx! {
        div { class: "relative pl-8 pb-8 last:pb-0",
            span { class: "absolute -left-[7px] top-1 w-3 h-3 rounded-full bg-gold" }
            div { class: "flex justify-between items-start gap-4",
                div {
                    p { class: "m-0", "{label}" }
                    p { class: "text-text-tertiary text-xs m-0 mt-1", "{date(entry_date)}" }
                    if let Some(n) = &notes {
                        p { class: "text-text-secondary text-sm m-0 mt-2", "{n}" }
                    }
                }
                div { class: "flex items-center gap-3 shrink-0",
                    p { class: amount_class, "{amount}" }
                    if edit_mode {
                        if let Some(id) = edit_id {
                            button {
                                class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                                onclick: move |_| on_edit.call(id),
                                "Edit"
                            }
                        }
                    }
                }
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

/// The card-identity edit form's raw text inputs, grouped for the same
/// reason as `HoldingEditInputs` above.
#[derive(Clone, Debug, Default)]
struct CardEditInputs {
    card_number: String,
    player_name: String,
    variant: String,
    parallel_name: String,
    print_run: String,
    is_rookie: bool,
    is_autograph: bool,
    is_relic: bool,
    notes: String,
}

fn submit_card_edit(
    card_id: i64,
    inputs: CardEditInputs,
    repo: &Repository,
) -> CardRoiResult<Card> {
    use cardroi::error::CardRoiError;

    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    let print_run = parse_optional_i32(&inputs.print_run).map_err(CardRoiError::validation)?;
    let edit = CardEdit {
        card_number: inputs.card_number,
        player_name: inputs.player_name,
        variant: non_empty(inputs.variant),
        parallel_name: non_empty(inputs.parallel_name),
        print_run,
        is_rookie: inputs.is_rookie,
        is_autograph: inputs.is_autograph,
        is_relic: inputs.is_relic,
        notes: non_empty(inputs.notes),
    };
    repo.update_card(card_id, &edit)
}

/// Corrects a card's own catalog identity (player, number, variant/
/// parallel/print run, rookie/autograph/relic, notes) - not which
/// holding it is or that holding's own grading/serial details (those are
/// `HoldingEditForm`'s job, right above this toggle). Affects every
/// holding that references this card, correctly, since they're all the
/// same catalog print (see `cardroi::models::CardEdit`'s doc comment).
#[component]
fn CardEditForm(card: Card, on_saved: EventHandler<()>) -> Element {
    let bridge = use_context::<WebBridge>();
    let card_id = card.id;
    let card_number_input = use_signal(|| card.card_number.clone());
    let player_name_input = use_signal(|| card.player_name.clone());
    let variant_input = use_signal(|| card.variant.clone().unwrap_or_default());
    let parallel_name_input = use_signal(|| card.parallel_name.clone().unwrap_or_default());
    let print_run_input = use_signal(|| card.print_run.map(|r| r.to_string()).unwrap_or_default());
    let mut is_rookie = use_signal(|| card.is_rookie);
    let mut is_autograph = use_signal(|| card.is_autograph);
    let mut is_relic = use_signal(|| card.is_relic);
    let notes_input = use_signal(|| card.notes.clone().unwrap_or_default());
    let mut error = use_signal(|| None::<String>);

    let submit = move |_| {
        let bridge = bridge.clone();
        let inputs = CardEditInputs {
            card_number: card_number_input(),
            player_name: player_name_input(),
            variant: variant_input(),
            parallel_name: parallel_name_input(),
            print_run: print_run_input(),
            is_rookie: is_rookie(),
            is_autograph: is_autograph(),
            is_relic: is_relic(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_card_edit(card_id, inputs, repo))
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
                FormField { label: "Player", value: player_name_input, placeholder: "e.g. LeBron James" }
                FormField { label: "Card number", value: card_number_input, placeholder: "e.g. 123" }
                FormField { label: "Variant", value: variant_input, placeholder: "e.g. Refractor" }
                FormField { label: "Parallel", value: parallel_name_input, placeholder: "e.g. Gold" }
                FormField { label: "Print run", value: print_run_input, placeholder: "e.g. 25" }
                FormField { label: "Notes", value: notes_input, placeholder: "" }
            }
            div { class: "flex flex-wrap gap-4",
                label { class: "flex items-center gap-2 text-text-secondary text-sm",
                    input {
                        r#type: "checkbox",
                        checked: is_rookie(),
                        onchange: move |evt| is_rookie.set(evt.checked()),
                    }
                    "Rookie"
                }
                label { class: "flex items-center gap-2 text-text-secondary text-sm",
                    input {
                        r#type: "checkbox",
                        checked: is_autograph(),
                        onchange: move |evt| is_autograph.set(evt.checked()),
                    }
                    "Autograph"
                }
                label { class: "flex items-center gap-2 text-text-secondary text-sm",
                    input {
                        r#type: "checkbox",
                        checked: is_relic(),
                        onchange: move |evt| is_relic.set(evt.checked()),
                    }
                    "Relic"
                }
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
    fn submit_card_edit_corrects_the_catalog_entry_and_leaves_other_fields_alone() {
        let (repo, holding_id) = repo_with_owned_holding();
        let card_id = repo.get_holding(holding_id).unwrap().card_id;

        let updated = submit_card_edit(
            card_id,
            CardEditInputs {
                card_number: "1".to_string(),
                player_name: "Test Player".to_string(),
                variant: "Refractor".to_string(),
                parallel_name: "Gold".to_string(),
                print_run: "25".to_string(),
                is_rookie: true,
                is_autograph: false,
                is_relic: false,
                notes: String::new(),
            },
            &repo,
        )
        .unwrap();

        assert_eq!(updated.variant.as_deref(), Some("Refractor"));
        assert_eq!(updated.parallel_name.as_deref(), Some("Gold"));
        assert_eq!(updated.print_run, Some(25));
        assert!(updated.is_rookie);
        // Confirmed via the real repository round-trip, not just the
        // returned value - proves the row itself was actually updated.
        assert_eq!(repo.get_card(card_id).unwrap(), updated);
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
}
