//! The Portfolio screen: the Sharesight-style "regroup the same table in
//! place" pattern (by set/player/sport), with hard-required pagination -
//! `Repository::list_holdings_page`/`count_holdings_page` fetch one page
//! of rows at a time via real SQL `LIMIT`/`OFFSET`, so the DOM only ever
//! holds one page's worth of rows regardless of portfolio size.
//! `HoldingTableRow` renders two physical layouts (a dense grid at
//! desktop widths, a stacked card at phone widths, toggled via
//! `hidden md:grid` / `flex md:hidden`, the same responsive pattern the
//! nav shell uses) rather than one fixed grid, which would be illegible
//! on a phone.
//!
//! Grouping is a two-step drill: picking a dimension shows a small
//! "groups index" (one row per distinct set/player/sport, each with a
//! `RollupPnl` subtotal via `analytics::portfolio`'s attribution
//! functions), then selecting one group filters the paginated table down
//! to it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cardroi::analytics::portfolio;
use cardroi::analytics::roi::{self, RollupPnl};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{HoldingStatus, Money};
use chrono::NaiveDate;
use dioxus::prelude::*;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::ld_icons::LdImage;

use crate::routes::Route;
use crate::web_bridge::WebBridge;

use super::format::{date, money};

const PAGE_SIZE: i64 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupBy {
    None,
    Set,
    Player,
    Sport,
}

impl GroupBy {
    fn label(self) -> &'static str {
        match self {
            GroupBy::None => "All",
            GroupBy::Set => "Set",
            GroupBy::Player => "Player",
            GroupBy::Sport => "Sport",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct GroupSummary {
    label: String,
    pnl: RollupPnl,
}

#[derive(Clone, Debug, PartialEq)]
struct HoldingRow {
    holding_id: i64,
    card_name: String,
    set_name: String,
    status: HoldingStatus,
    acquired_date: Option<NaiveDate>,
    cost_basis: Money,
    realized_pnl: Option<Money>,
    unrealized_pnl: Option<Money>,
    /// The primary photo's small thumbnail, if one exists - fetched via
    /// `get_primary_thumbnail` (thumbnail bytes only), never
    /// `list_photos_for_holding` (which would also pull each row's
    /// potentially much larger `full_data` BLOB for data never shown
    /// here).
    thumbnail: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
struct HoldingsPage {
    rows: Vec<HoldingRow>,
    total_count: i64,
    page: i64,
}

#[derive(Clone, Debug, PartialEq)]
enum ViewData {
    Groups(Vec<GroupSummary>),
    Page(HoldingsPage),
}

fn load_group_summary(group_by: GroupBy, repo: &Repository) -> CardRoiResult<Vec<GroupSummary>> {
    let entries = match group_by {
        GroupBy::None => Vec::new(),
        GroupBy::Set => portfolio::attribution_by_set(repo)?,
        GroupBy::Player => portfolio::attribution_by_player(repo)?,
        GroupBy::Sport => portfolio::attribution_by_sport(repo)?,
    };
    Ok(entries
        .into_iter()
        .map(|e| GroupSummary {
            label: e.label,
            pnl: e.pnl,
        })
        .collect())
}

fn load_holdings_page(
    group_by: GroupBy,
    selected_group: Option<String>,
    page: i64,
    repo: &Repository,
) -> CardRoiResult<HoldingsPage> {
    let (set_id, player_name, sport) = match (group_by, selected_group.as_deref()) {
        (GroupBy::Set, Some(label)) => {
            let set_id = repo
                .list_sets()?
                .into_iter()
                .find(|s| s.name == label)
                .map(|s| s.id);
            (set_id, None, None)
        }
        (GroupBy::Player, Some(label)) => (None, Some(label.to_string()), None),
        (GroupBy::Sport, Some(label)) => (None, None, Some(label.to_string())),
        _ => (None, None, None),
    };

    let offset = page * PAGE_SIZE;
    let holdings = repo.list_holdings_page(
        None,
        set_id,
        player_name.as_deref(),
        sport.as_deref(),
        PAGE_SIZE,
        offset,
    )?;
    let total_count =
        repo.count_holdings_page(None, set_id, player_name.as_deref(), sport.as_deref())?;

    let mut rows = Vec::with_capacity(holdings.len());
    for holding in &holdings {
        let card = repo.get_card(holding.card_id)?;
        let set = repo.get_set(card.set_id)?;
        let pnl = roi::holding_pnl(repo, holding.id)?;
        let thumbnail = repo.get_primary_thumbnail(holding.id)?;
        rows.push(HoldingRow {
            holding_id: holding.id,
            card_name: card.display_name(),
            set_name: set.name,
            status: holding.status,
            acquired_date: holding.acquired_date,
            cost_basis: pnl.cost_basis,
            realized_pnl: pnl.realized_pnl,
            unrealized_pnl: pnl.unrealized_pnl,
            thumbnail,
        });
    }

    Ok(HoldingsPage {
        rows,
        total_count,
        page,
    })
}

#[component]
pub fn Portfolio() -> Element {
    let bridge = use_context::<WebBridge>();
    let mut group_by = use_signal(|| GroupBy::None);
    let mut selected_group = use_signal(|| None::<String>);
    let mut page = use_signal(|| 0i64);

    let data = use_resource(move || {
        let bridge = bridge.clone();
        let group_by = group_by();
        let selected_group = selected_group();
        let page = page();
        async move {
            bridge
                .run(move |repo| {
                    if group_by != GroupBy::None && selected_group.is_none() {
                        load_group_summary(group_by, repo).map(ViewData::Groups)
                    } else {
                        load_holdings_page(group_by, selected_group, page, repo).map(ViewData::Page)
                    }
                })
                .await
        }
    });

    let mut set_group_by = move |next: GroupBy| {
        group_by.set(next);
        selected_group.set(None);
        page.set(0);
    };

    rsx! {
        div { class: "p-8 flex flex-col gap-6 max-w-5xl",
            h1 { class: "text-2xl font-semibold m-0", "Portfolio" }

            div { class: "flex gap-2",
                for option in [GroupBy::None, GroupBy::Set, GroupBy::Player, GroupBy::Sport] {
                    button {
                        class: if option == group_by() {
                            "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer"
                        } else {
                            "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer"
                        },
                        onclick: move |_| set_group_by(option),
                        "{option.label()}"
                    }
                }
            }

            if let Some(label) = selected_group() {
                div { class: "flex items-center gap-2 text-sm",
                    button {
                        class: "text-text-secondary underline bg-transparent border-none cursor-pointer p-0",
                        onclick: move |_| {
                            selected_group.set(None);
                            page.set(0);
                        },
                        "{group_by().label()}"
                    }
                    span { class: "text-text-tertiary", "/" }
                    span { "{label}" }
                }
            }

            match &*data.read() {
                None => rsx! {
                    div { class: "text-text-secondary", "Loading..." }
                },
                Some(Err(err)) => rsx! {
                    div { class: "text-loss", "Failed to load portfolio data: {err}" }
                },
                Some(Ok(ViewData::Groups(groups))) => rsx! {
                    GroupsIndex {
                        groups: groups.clone(),
                        on_select: move |label: String| {
                            selected_group.set(Some(label));
                            page.set(0);
                        },
                    }
                },
                Some(Ok(ViewData::Page(holdings_page))) => rsx! {
                    HoldingsTable {
                        page: holdings_page.clone(),
                        on_first: move |_| page.set(0),
                        on_prev: move |_| page.set((page() - 1).max(0)),
                        on_next: move |_| page.set(page() + 1),
                        on_last: {
                            let total_count = holdings_page.total_count;
                            move |_| {
                                let total_pages = ((total_count + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
                                page.set(total_pages - 1);
                            }
                        },
                    }
                },
            }
        }
    }
}

#[component]
fn GroupsIndex(groups: Vec<GroupSummary>, on_select: EventHandler<String>) -> Element {
    if groups.is_empty() {
        return rsx! {
            div { class: "flex flex-col gap-3 items-start",
                p { class: "text-text-secondary m-0", "Nothing to group yet - buy your first card to start building your portfolio." }
                Link {
                    to: Route::BuyRoute {},
                    class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none no-underline font-semibold cursor-pointer",
                    "Log your first buy"
                }
            }
        };
    }
    rsx! {
        div { class: "flex flex-col",
            for group in groups {
                div {
                    class: "flex justify-between items-center py-3 border-b border-border cursor-pointer hover:bg-surface",
                    onclick: {
                        let label = group.label.clone();
                        move |_| on_select.call(label.clone())
                    },
                    div {
                        p { class: "m-0", "{group.label}" }
                        p { class: "text-text-tertiary text-xs m-0 mt-1",
                            "{group.pnl.holding_count} holdings ({group.pnl.closed_count} closed)"
                        }
                    }
                    div { class: "text-right",
                        p { class: "data-numeral m-0", "{money(group.pnl.cost_basis)}" }
                        p { class: "data-numeral text-xs m-0 mt-1 text-text-tertiary",
                            "realized {money(group.pnl.realized_pnl)}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HoldingsTable(
    page: HoldingsPage,
    on_first: EventHandler<()>,
    on_prev: EventHandler<()>,
    on_next: EventHandler<()>,
    on_last: EventHandler<()>,
) -> Element {
    let total_pages = ((page.total_count + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let current_page_number = page.page + 1;

    if page.rows.is_empty() {
        if page.total_count == 0 {
            return rsx! {
                div { class: "flex flex-col gap-3 items-start",
                    p { class: "text-text-secondary m-0", "No holdings recorded yet. Every card you log will show up here." }
                    Link {
                        to: Route::BuyRoute {},
                        class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none no-underline font-semibold cursor-pointer",
                        "Log your first buy"
                    }
                }
            };
        }
        return rsx! {
            p { class: "text-text-secondary m-0", "No holdings match this view." }
        };
    }

    rsx! {
        div { class: "flex flex-col",
            // Column headers - desktop grid layout only, the phone
            // stacked-card layout below labels each field inline instead.
            div { class: "hidden md:grid gap-4 py-2 border-b border-border text-text-tertiary text-xs uppercase tracking-wide",
                style: "grid-template-columns: 2fr 1.5fr 1fr 1fr 1fr;",
                span { "Card" }
                span { "Set" }
                span { "Status" }
                span { "Acquired" }
                span { class: "text-right", "Cost basis / P&L" }
            }
            for row in page.rows {
                HoldingTableRow { row }
            }
        }

        div { class: "flex justify-between items-center pt-2",
            span { class: "text-text-secondary text-sm",
                "Page {current_page_number} of {total_pages} ({page.total_count} holdings)"
            }
            div { class: "flex gap-2",
                // Previous/Next alone means 200+ clicks to reach the last
                // page on a large portfolio, so First/Last are worth the
                // extra two buttons.
                button {
                    class: "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer disabled:opacity-40",
                    disabled: page.page == 0,
                    onclick: move |_| if page.page > 0 { on_first.call(()) },
                    "First"
                }
                button {
                    class: "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer disabled:opacity-40",
                    disabled: page.page == 0,
                    // Guarding the click itself (not just the `disabled`
                    // attribute) closes a double-click race: the attribute
                    // only updates once the next render lands, which lags
                    // a tick behind the click during the async refetch, so
                    // two fast clicks could both fire before it catches up.
                    onclick: move |_| if page.page > 0 { on_prev.call(()) },
                    "Previous"
                }
                button {
                    class: "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer disabled:opacity-40",
                    disabled: current_page_number >= total_pages,
                    onclick: move |_| if current_page_number < total_pages { on_next.call(()) },
                    "Next"
                }
                button {
                    class: "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer disabled:opacity-40",
                    disabled: current_page_number >= total_pages,
                    onclick: move |_| if current_page_number < total_pages { on_last.call(()) },
                    "Last"
                }
            }
        }
    }
}

/// A small avatar-sized rendering of a holding's primary-photo
/// thumbnail, or the same neutral placeholder Card Details' hero uses
/// when there isn't one - one consistent "this is a photo slot" idiom
/// across the app rather than a screen-specific empty state.
#[component]
fn RowThumbnail(thumbnail: Option<Vec<u8>>) -> Element {
    rsx! {
        div { class: "w-8 h-8 shrink-0 rounded-md overflow-hidden bg-surface-elevated flex items-center justify-center",
            if let Some(bytes) = &thumbnail {
                img {
                    class: "w-full h-full object-cover",
                    src: "data:image/jpeg;base64,{BASE64.encode(bytes)}",
                }
            } else {
                Icon { icon: LdImage, width: 14, height: 14, class: "text-text-tertiary opacity-50" }
            }
        }
    }
}

#[component]
fn HoldingTableRow(row: HoldingRow) -> Element {
    let acquired = row
        .acquired_date
        .map(date)
        .unwrap_or_else(|| "-".to_string());
    let pnl_class = |m: Money| {
        if m.is_negative() {
            "text-loss"
        } else {
            "text-gain"
        }
    };

    rsx! {
        Link {
            to: Route::HoldingDetailRoute { id: row.holding_id },
            class: "block border-b border-border no-underline text-text-primary hover:bg-surface",

            // Desktop: dense 5-column grid row. The thumbnail is an
            // avatar prepended to the name cell, not its own grid column
            // - adding a column would disturb every other row's
            // alignment for a decoration, not new information.
            div {
                class: "hidden md:grid gap-4 py-3 items-center",
                style: "grid-template-columns: 2fr 1.5fr 1fr 1fr 1fr;",
                div { class: "flex items-center gap-2 min-w-0",
                    RowThumbnail { thumbnail: row.thumbnail.clone() }
                    span { class: "truncate", "{row.card_name}" }
                }
                span { class: "text-text-secondary", "{row.set_name}" }
                span { class: "text-text-secondary", "{row.status.as_str()}" }
                span { class: "text-text-secondary", "{acquired}" }
                div { class: "text-right",
                    p { class: "data-numeral m-0", "{money(row.cost_basis)}" }
                    if let Some(realized) = row.realized_pnl {
                        p { class: "data-numeral text-xs m-0 mt-1 {pnl_class(realized)}", "realized {money(realized)}" }
                    } else if let Some(unrealized) = row.unrealized_pnl {
                        p { class: "data-numeral text-xs m-0 mt-1 {pnl_class(unrealized)}", "unrealized {money(unrealized)}" }
                    }
                }
            }

            // Phone: stacked card - a 5-column grid crammed into a phone
            // screen would be illegible, so this is a genuinely different
            // layout, not a shrunk copy of the desktop one.
            div {
                class: "flex md:hidden flex-col gap-1 py-3",
                div { class: "flex justify-between items-baseline gap-2",
                    div { class: "flex items-center gap-2 min-w-0",
                        RowThumbnail { thumbnail: row.thumbnail.clone() }
                        span { class: "font-medium truncate", "{row.card_name}" }
                    }
                    span { class: "data-numeral shrink-0", "{money(row.cost_basis)}" }
                }
                div { class: "flex justify-between items-baseline gap-2 text-text-secondary text-xs",
                    span { class: "truncate", "{row.set_name} · {row.status.as_str()} · {acquired}" }
                    if let Some(realized) = row.realized_pnl {
                        span { class: "data-numeral shrink-0 {pnl_class(realized)}", "realized {money(realized)}" }
                    } else if let Some(unrealized) = row.unrealized_pnl {
                        span { class: "data-numeral shrink-0 {pnl_class(unrealized)}", "unrealized {money(unrealized)}" }
                    }
                }
            }
        }
    }
}
