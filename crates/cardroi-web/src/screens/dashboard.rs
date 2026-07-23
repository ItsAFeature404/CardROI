//! The home screen: an Action Center, not a report. Real research
//! (Reddit patterns plus direct evidence from CollX/Card Ladder/Ludex
//! reviews) confirmed collectors open a collection app for one of four
//! recurring jobs - capture something, find something, see what
//! changed, or go deeper - not for a single emotional moment. Card
//! Details already owns emotional (one ownership's story); this screen
//! owns effective.
//!
//! Search and the capture actions (Buy/Sell/Comp) are deliberately one
//! visual unit, not two - both are just ways to begin work. Everything
//! else (headline value, attention, notable mover, newest addition)
//! renders below, quieter, answering "what changed" for whoever has
//! more than a few seconds - "go deeper" needs no content of its own
//! here at all, since the persistent nav already does that job on every
//! screen.
//!
//! This screen's emotional space stays orientation only - it never
//! delivers hard news. The notable-mover line is deliberately
//! gains-only; a holding that's down significantly belongs on that
//! holding's own page, not here (see CLAUDE.md's "Emotional spaces"
//! section). "Total P&L vs. cost basis" stands in for a conventional
//! dashboard's day-over-day delta line: CardROI stores no periodic
//! portfolio-value snapshots, so a literal "+2.3% today" figure isn't
//! something this data model can honestly compute yet - this is the
//! same cost-basis-relative math the CLI already reports (`cardroi
//! roi`), just surfaced as the headline delta instead of a time-based
//! change.

use cardroi::analytics::roi::{RollupPnl, holding_pnl, portfolio_pnl};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{Holding, HoldingStatus, Money};
use chrono::{DateTime, NaiveDate, Utc};
use dioxus::prelude::*;
use rust_decimal::Decimal;

use crate::components::holding_picker::{HoldingOption, load_holding_options};
use crate::local_prefs;
use crate::routes::Route;
use crate::web_bridge::WebBridge;

use super::format::{money, percent};

/// An owned holding's latest comp counts as fresh inside this window;
/// past it, it's treated the same as never having been comped at all.
/// Card prices genuinely move over a quarter, not week to week - shorter
/// would nag over noise, longer would let real drift go unnoticed.
const STALE_COMP_DAYS: i64 = 90;
/// The notable-mover line only ever fires for a genuine double-or-better
/// (a 12% gain is real but not what this line means). `Decimal::ONE` is
/// a 100% unrealized ROI (`unrealized_pnl / cost_basis`).
const NOTABLE_MOVER_MIN_ROI: Decimal = Decimal::ONE;
/// "Your newest addition" stops being news past this many days.
const RECENT_ADDITION_WINDOW_DAYS: i64 = 14;
/// This many or more holdings created within `BULK_IMPORT_WINDOW_SECONDS`
/// of each other reads as a bulk import, not a personal moment - there's
/// no honest way to pick just one of a batch to name.
const BULK_IMPORT_BATCH_THRESHOLD: usize = 3;
const BULK_IMPORT_WINDOW_SECONDS: i64 = 5;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AttentionStatus {
    /// Not a single owned holding has ever been comped - a different,
    /// more foundational message than "some are stale."
    NonePricedYet,
    /// Every owned holding has a comp within the freshness window.
    AllFresh,
    NeedsComps {
        count: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MoverItem {
    holding_id: i64,
    card_name: String,
    unrealized_pnl: Money,
    unrealized_roi_pct: Decimal,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewestAddition {
    card_name: String,
    logged_days_ago: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NextAction {
    label: String,
    route: Route,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DashboardData {
    pub(crate) rollup: RollupPnl,
    pub(crate) needs_attention: AttentionStatus,
    pub(crate) notable_mover: Option<MoverItem>,
    pub(crate) newest_addition: Option<NewestAddition>,
    pub(crate) next_actions: Vec<NextAction>,
}

/// Picks the single owned holding most worth naming in a "review this"
/// next-action: a never-priced holding first (found in iteration order),
/// else the one with the oldest stale comp. Tracked alongside the
/// attention-count loop below rather than as a second pass.
#[derive(Clone, Copy)]
struct AttentionCandidate {
    holding_id: i64,
    card_id: i64,
    /// `None` ranks ahead of any `Some` date - never-priced is more
    /// urgent than merely-stale.
    oldest_comp_date: Option<NaiveDate>,
}

/// Finds the most recently logged holding to name as "your newest
/// addition," or `None` if there isn't a clean single answer - either
/// nothing was logged recently enough to be news, or several holdings
/// were created within the same few seconds (a bulk import, not a
/// personal moment - picking one to name would be a guess).
fn find_newest_addition(owned: &[Holding], today: NaiveDate) -> Option<&Holding> {
    let newest = owned.iter().max_by_key(|h| h.created_at)?;

    let batch_size = owned
        .iter()
        .filter(|h| {
            (h.created_at - newest.created_at).num_seconds().abs() <= BULK_IMPORT_WINDOW_SECONDS
        })
        .count();
    if batch_size >= BULK_IMPORT_BATCH_THRESHOLD {
        return None;
    }

    let days_ago = (today - newest.created_at.date_naive()).num_days();
    (0..=RECENT_ADDITION_WINDOW_DAYS)
        .contains(&days_ago)
        .then_some(newest)
}

/// "today" / "yesterday" / "N days ago" - `RECENT_ADDITION_WINDOW_DAYS`
/// already guarantees this is never a large number.
fn logged_recency_phrase(days_ago: i64) -> String {
    match days_ago {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        n => format!("{n} days ago"),
    }
}

/// Runs against the real `Repository` through the web bridge. One extra
/// query per owned holding (`holding_pnl`, resolving its unrealized P&L
/// and comp date) plus at most two more (the notable mover's and newest
/// addition's card names) - the same N+1 shape `analytics::roi::rollup`
/// already accepts at this project's scale, not a new performance risk.
pub(crate) fn load_dashboard_data(repo: &Repository) -> CardRoiResult<DashboardData> {
    let rollup = portfolio_pnl(repo)?;
    let owned = repo.list_holdings(None, Some(HoldingStatus::Owned))?;

    let today = Utc::now().date_naive();
    let stale_cutoff = today - chrono::Duration::days(STALE_COMP_DAYS);

    let mut never_priced_count = 0usize;
    let mut needs_comp_count = 0usize;
    let mut attention_candidate: Option<AttentionCandidate> = None;
    let mut best_mover: Option<(i64, i64, Money, Decimal)> = None; // (holding_id, card_id, pnl, roi)

    for holding in &owned {
        let pnl = holding_pnl(repo, holding.id)?;

        let stale = match pnl.unrealized_pnl_as_of {
            None => {
                never_priced_count += 1;
                true
            }
            Some(as_of) if as_of < stale_cutoff => true,
            Some(_) => false,
        };
        if stale {
            needs_comp_count += 1;
            let is_more_urgent = match attention_candidate {
                None => true,
                Some(existing) => match (existing.oldest_comp_date, pnl.unrealized_pnl_as_of) {
                    (None, _) => false, // an existing never-priced candidate always wins
                    (Some(_), None) => true,
                    (Some(existing_date), Some(this_date)) => this_date < existing_date,
                },
            };
            if is_more_urgent {
                attention_candidate = Some(AttentionCandidate {
                    holding_id: holding.id,
                    card_id: holding.card_id,
                    oldest_comp_date: pnl.unrealized_pnl_as_of,
                });
            }
        }

        if let (Some(unrealized_pnl), Some(roi)) = (pnl.unrealized_pnl, pnl.unrealized_roi_pct)
            && roi >= NOTABLE_MOVER_MIN_ROI
        {
            let is_best = best_mover.is_none_or(|(_, _, _, best_roi)| roi > best_roi);
            if is_best {
                best_mover = Some((holding.id, holding.card_id, unrealized_pnl, roi));
            }
        }
    }

    let needs_attention = if needs_comp_count == 0 {
        AttentionStatus::AllFresh
    } else if never_priced_count == owned.len() {
        AttentionStatus::NonePricedYet
    } else {
        AttentionStatus::NeedsComps {
            count: needs_comp_count,
        }
    };

    let notable_mover = match best_mover {
        Some((holding_id, card_id, unrealized_pnl, unrealized_roi_pct)) => {
            let card = repo.get_card(card_id)?;
            Some(MoverItem {
                holding_id,
                card_name: card.display_name(),
                unrealized_pnl,
                unrealized_roi_pct,
            })
        }
        None => None,
    };

    let newest_addition_holding = find_newest_addition(&owned, today);
    let newest_addition = newest_addition_holding
        .map(|holding| -> CardRoiResult<NewestAddition> {
            let card = repo.get_card(holding.card_id)?;
            Ok(NewestAddition {
                card_name: card.display_name(),
                logged_days_ago: (today - holding.created_at.date_naive()).num_days(),
            })
        })
        .transpose()?;

    // Priority order, capped at two: review what needs attention, then
    // the notable mover, then the one action that's always safe. Never
    // depends on Portfolio filtering, which doesn't exist yet - both
    // "review" actions link straight to a specific holding.
    let mut next_actions = Vec::with_capacity(2);
    if let Some(candidate) = attention_candidate {
        let card = repo.get_card(candidate.card_id)?;
        next_actions.push(NextAction {
            label: format!("Review {}", card.display_name()),
            route: Route::HoldingDetailRoute {
                id: candidate.holding_id,
            },
        });
    }
    if let Some(mover) = &notable_mover
        && next_actions.len() < 2
    {
        next_actions.push(NextAction {
            label: format!("Look at {}", mover.card_name),
            route: Route::HoldingDetailRoute {
                id: mover.holding_id,
            },
        });
    }
    if next_actions.is_empty() {
        next_actions.push(NextAction {
            label: "Log a new buy".to_string(),
            route: Route::BuyRoute {},
        });
    }

    Ok(DashboardData {
        rollup,
        needs_attention,
        notable_mover,
        newest_addition,
        next_actions,
    })
}

fn greeting_for(now: DateTime<Utc>, name: Option<&str>) -> String {
    // Client-side only, no repository call - this app has no server to
    // ask, so "now" is whatever clock the browser reports. Standard
    // morning/afternoon/evening boundaries.
    let hour = now.format("%H").to_string().parse::<u32>().unwrap_or(12);
    let time_of_day = if hour < 12 {
        "Good morning"
    } else if hour < 18 {
        "Good afternoon"
    } else {
        "Good evening"
    };
    match name {
        Some(name) if !name.trim().is_empty() => format!("{time_of_day}, {}.", name.trim()),
        _ => format!("{time_of_day}."),
    }
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

/// The one-time "what should I call you" moment. Fires under both
/// conditions in the design (a brand-new empty collection, or an
/// existing one that predates this ever being asked) via the same
/// `has_prompted_for_name` check - answering or skipping both
/// permanently end it, so it never asks twice either way.
#[component]
fn NamePrompt(on_done: EventHandler<()>) -> Element {
    let mut name_input = use_signal(String::new);

    let submit = move |_| {
        let name = name_input();
        if !name.trim().is_empty() {
            local_prefs::set_collector_name(&name);
        }
        local_prefs::mark_name_prompted();
        on_done.call(());
    };
    let skip = move |_| {
        local_prefs::mark_name_prompted();
        on_done.call(());
    };

    rsx! {
        div { class: "flex flex-col gap-3 p-4 bg-surface rounded-radius",
            p { class: "m-0 font-semibold", "Welcome to CardROI. What should I call you?" }
            div { class: "flex gap-2 flex-wrap items-center",
                input {
                    class: "bg-canvas text-text-primary border border-border rounded-radius px-2 py-1.5 font-data",
                    placeholder: "Your name",
                    value: "{name_input}",
                    oninput: move |evt| name_input.set(evt.value()),
                }
                button {
                    class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer",
                    onclick: submit,
                    "Save"
                }
                button {
                    class: "px-3 py-2 rounded-radius bg-transparent text-text-secondary border border-border cursor-pointer",
                    onclick: skip,
                    "Skip for now"
                }
            }
        }
    }
}

#[component]
fn DashboardBody(data: DashboardData) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut show_name_prompt = use_signal(|| !local_prefs::has_prompted_for_name());
    let mut collector_name = use_signal(local_prefs::collector_name);
    let greeting = greeting_for(Utc::now(), collector_name().as_deref());
    let on_name_done = move |_| {
        collector_name.set(local_prefs::collector_name());
        show_name_prompt.set(false);
    };

    // Search: "do I already own this," confirmed by real research as
    // one of the four recurring reasons a collector opens this app.
    // Fetch-all-then-filter-client-side, the same pattern already
    // proven at this app's scale by `CardPicker`/`HoldingPicker` -
    // reused, not reinvented. Navigating to a result (not selecting it
    // into a form) is genuinely different from every existing use of
    // this data, so it gets its own small block rather than reusing
    // `HoldingPicker` itself.
    let mut search_query = use_signal(String::new);
    let search_options = use_resource({
        let bridge = bridge.clone();
        move || {
            let bridge = bridge.clone();
            async move { bridge.run(|repo| load_holding_options(None, repo)).await }
        }
    });

    if data.rollup.holding_count == 0 {
        return rsx! {
            div { class: "p-8 flex flex-col gap-6 max-w-2xl",
                p { class: "data-numeral text-2xl m-0", "{greeting}" }
                if show_name_prompt() {
                    NamePrompt { on_done: on_name_done }
                }
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

    let attention_line = match data.needs_attention {
        AttentionStatus::AllFresh => "Every card you own has a recent comp on record.".to_string(),
        AttentionStatus::NonePricedYet => {
            "None of your cards have a recorded value yet.".to_string()
        }
        AttentionStatus::NeedsComps { count: 1 } => "1 card needs a fresh comp.".to_string(),
        AttentionStatus::NeedsComps { count } => format!("{count} cards need fresh comps."),
    };

    // Only computed once there's an actual query - an idle search box
    // shouldn't force a render pass over the whole holding list.
    let query_trimmed = search_query();
    let query_trimmed = query_trimmed.trim();
    let search_matches: Option<Vec<HoldingOption>> = if query_trimmed.is_empty() {
        None
    } else if let Some(Ok(options)) = search_options.read().as_ref() {
        let query_lower = query_trimmed.to_lowercase();
        Some(
            options
                .iter()
                .filter(|o| o.label.to_lowercase().contains(&query_lower))
                .take(8)
                .cloned()
                .collect(),
        )
    } else {
        None
    };

    rsx! {
        div { class: "p-8 flex flex-col gap-8 max-w-2xl",
            p { class: "data-numeral text-2xl m-0", "{greeting}" }
            if show_name_prompt() {
                NamePrompt { on_done: on_name_done }
            }

            // The Action Center: search and capture are both just ways
            // to begin work, so they're one visual unit, not two - the
            // hero of the page in the sense of friction-free action,
            // never a card or a number.
            div { class: "rounded-[20px] bg-surface p-6 flex flex-col gap-4",
                div {
                    input {
                        class: "bg-canvas text-text-primary border border-border rounded-radius px-3 py-2 font-data w-full",
                        placeholder: "Search your collection...",
                        value: "{search_query}",
                        oninput: move |evt| search_query.set(evt.value()),
                    }
                    if let Some(matches) = &search_matches {
                        div { class: "flex flex-col mt-2", style: "max-height: 240px; overflow-y: auto;",
                            if matches.is_empty() {
                                p { class: "text-text-secondary text-sm m-0 mt-1", "No matches." }
                            } else {
                                for option in matches.iter().cloned() {
                                    Link {
                                        key: "{option.holding_id}",
                                        to: Route::HoldingDetailRoute { id: option.holding_id },
                                        class: "flex justify-between items-center px-2 py-1.5 rounded-radius no-underline text-text-primary hover:bg-surface-elevated",
                                        span { "{option.label}" }
                                        span { class: "text-text-tertiary text-xs", "{option.status.as_str()}" }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "flex gap-2 flex-wrap pt-2 border-t border-border",
                    Link {
                        to: Route::BuyRoute {},
                        class: "px-4 py-2 rounded-radius bg-surface-elevated text-text-primary border border-border no-underline font-semibold cursor-pointer hover:bg-gold hover:text-canvas hover:border-transparent transition-colors duration-[var(--duration-standard)] ease-standard",
                        "Log Buy"
                    }
                    Link {
                        to: Route::SellRoute {},
                        class: "px-4 py-2 rounded-radius bg-surface-elevated text-text-primary border border-border no-underline font-semibold cursor-pointer hover:bg-gold hover:text-canvas hover:border-transparent transition-colors duration-[var(--duration-standard)] ease-standard",
                        "Log Sell"
                    }
                    Link {
                        to: Route::CompRoute {},
                        class: "px-4 py-2 rounded-radius bg-surface-elevated text-text-primary border border-border no-underline font-semibold cursor-pointer hover:bg-gold hover:text-canvas hover:border-transparent transition-colors duration-[var(--duration-standard)] ease-standard",
                        "Add Comp"
                    }
                }
            }

            div { class: "flex flex-col gap-2 text-sm text-text-secondary",
                p { class: "m-0",
                    "Your collection is worth "
                    span { class: "data-numeral text-text-primary", "{money(total_value)}" }
                    ", based on your latest recorded research."
                }
                p { class: "data-numeral m-0 {pnl_class}",
                    "{sign}{money(total_pnl)} since cost basis"
                    if let Some(pct) = total_pnl_pct {
                        " ({percent(pct)})"
                    }
                }
                if rollup.appraised_open_count < open_count {
                    p { class: "text-text-tertiary text-xs m-0",
                        "Unrealized P&L reflects user-supplied comps, not live market values - {rollup.appraised_open_count}/{open_count} open holdings priced"
                    }
                }
                p { class: "m-0", "{attention_line}" }
                if let Some(mover) = &data.notable_mover {
                    p { class: "m-0",
                        "{mover.card_name} is up "
                        span { class: "data-numeral text-gain", "{percent(mover.unrealized_roi_pct)}" }
                        " since you bought it."
                    }
                }
                if let Some(addition) = &data.newest_addition {
                    p { class: "m-0",
                        "You logged {addition.card_name} {logged_recency_phrase(addition.logged_days_ago)}."
                    }
                }
            }

            div { class: "flex flex-col gap-2 mt-2",
                p { class: "text-text-secondary text-sm m-0", "What do you want to look at today?" }
                div { class: "flex gap-2 flex-wrap",
                    for action in data.next_actions.iter().cloned() {
                        Link {
                            to: action.route.clone(),
                            class: "px-4 py-2 rounded-radius bg-surface text-text-primary border border-border no-underline font-semibold cursor-pointer hover:bg-surface-elevated",
                            "{action.label}"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use cardroi::db::open_in_memory;
    use cardroi::models::{
        HoldingStatus as HStatus, NewAppraisal, NewCard, NewHolding, NewSet, NewTransaction,
    };
    use chrono::{Duration, TimeZone};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    fn repo_with_card(repo: &Repository) -> i64 {
        let set = repo
            .create_set(&NewSet {
                name: "Test Set".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_card(&NewCard {
            set_id: set.id,
            card_number: "1".to_string(),
            player_name: "Test Player".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id
    }

    fn buy_holding(repo: &Repository, card_id: i64, price: &str) -> i64 {
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: Money::from_str(price).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap();
        holding.id
    }

    fn comp(repo: &Repository, holding_id: i64, value: &str, appraised_date: NaiveDate) {
        repo.create_appraisal(&NewAppraisal {
            holding_id,
            appraised_value: Money::from_str(value).unwrap(),
            appraised_date,
            source: None,
            notes: None,
        })
        .unwrap();
    }

    #[wasm_bindgen_test]
    fn attention_status_distinguishes_none_priced_from_some_stale_from_all_fresh() {
        let repo = Repository::new(open_in_memory().unwrap());
        let card_id = repo_with_card(&repo);
        let today = Utc::now().date_naive();

        // One holding, never priced - the foundational message, not the
        // generic "N stale" one.
        let h1 = buy_holding(&repo, card_id, "100.00");
        assert_eq!(
            load_dashboard_data(&repo).unwrap().needs_attention,
            AttentionStatus::NonePricedYet
        );

        // Priced today - flips to AllFresh.
        comp(&repo, h1, "150.00", today);
        assert_eq!(
            load_dashboard_data(&repo).unwrap().needs_attention,
            AttentionStatus::AllFresh
        );

        // A second holding, priced 100 days ago - past the 90-day
        // freshness window, so it's the one that needs attention now.
        let h2 = buy_holding(&repo, card_id, "200.00");
        comp(&repo, h2, "210.00", today - Duration::days(100));
        assert_eq!(
            load_dashboard_data(&repo).unwrap().needs_attention,
            AttentionStatus::NeedsComps { count: 1 }
        );
    }

    #[wasm_bindgen_test]
    fn notable_mover_requires_a_genuine_double_and_picks_the_single_best() {
        let repo = Repository::new(open_in_memory().unwrap());
        let card_id = repo_with_card(&repo);
        let today = Utc::now().date_naive();

        // Bought for 100, comped at 150 - a real 50% gain, below the
        // 100%-or-better bar this line requires.
        let h1 = buy_holding(&repo, card_id, "100.00");
        comp(&repo, h1, "150.00", today);
        assert!(
            load_dashboard_data(&repo).unwrap().notable_mover.is_none(),
            "a 50% gain shouldn't count as notable"
        );

        // Bought for 100, comped at 250 - a genuine 150% gain.
        let h2 = buy_holding(&repo, card_id, "100.00");
        comp(&repo, h2, "250.00", today);

        // Bought for 100, comped at 400 - an even bigger 300% gain,
        // which should win over h2's 150%.
        let h3 = buy_holding(&repo, card_id, "100.00");
        comp(&repo, h3, "400.00", today);

        let mover = load_dashboard_data(&repo)
            .unwrap()
            .notable_mover
            .expect("the 300% gain should be notable");
        assert_eq!(mover.holding_id, h3);
        assert_eq!(mover.unrealized_roi_pct, Decimal::from_str("3").unwrap());
    }

    fn holding_at(id: i64, created_at: DateTime<Utc>) -> Holding {
        Holding {
            id,
            card_id: 1,
            serial_number: None,
            grade: None,
            grading_company: None,
            cert_number: None,
            status: HStatus::Owned,
            acquired_date: None,
            disposed_date: None,
            notes: None,
            created_at,
            updated_at: created_at,
        }
    }

    #[wasm_bindgen_test]
    fn find_newest_addition_omits_a_bulk_import_batch() {
        let base = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let today = base.date_naive();

        let single = vec![holding_at(1, base)];
        assert_eq!(find_newest_addition(&single, today).map(|h| h.id), Some(1));

        // Three holdings created within a couple seconds of each other -
        // a bulk import, not a personal moment - so no single one is named.
        let batch = vec![
            holding_at(1, base),
            holding_at(2, base + Duration::seconds(1)),
            holding_at(3, base + Duration::seconds(2)),
        ];
        assert_eq!(find_newest_addition(&batch, today), None);
    }

    #[wasm_bindgen_test]
    fn find_newest_addition_stops_being_news_past_the_recency_window() {
        let base = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();

        let still_recent = vec![holding_at(1, base)];
        assert_eq!(
            find_newest_addition(
                &still_recent,
                base.date_naive() + Duration::days(RECENT_ADDITION_WINDOW_DAYS)
            )
            .map(|h| h.id),
            Some(1),
            "right at the edge of the window should still count"
        );

        let too_old = vec![holding_at(1, base)];
        assert_eq!(
            find_newest_addition(
                &too_old,
                base.date_naive() + Duration::days(RECENT_ADDITION_WINDOW_DAYS + 1)
            ),
            None,
            "one day past the window should no longer be news"
        );
    }
}
