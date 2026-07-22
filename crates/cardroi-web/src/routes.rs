//! Route table for the web shell. Every route renders inside `Shell`
//! (`components::nav`) via `#[layout(Shell)]` - the nav never re-mounts on
//! navigation, only the page body inside its `Outlet` swaps.
//!
//! Ledger and Settings are still placeholders; every other route is a
//! real screen. The full route set (Buy/Sell/Comp/holding detail, not
//! just the nav-visible destinations) is declared up front rather than
//! grown incrementally, since screens link to each other (e.g.
//! Dashboard's empty-state and top-movers links need `BuyRoute`/
//! `HoldingDetailRoute` regardless of which screen is built first).

use crate::components::nav::Shell;
use crate::screens::buy_form::BuyForm;
use crate::screens::comp_form::CompForm;
use crate::screens::dashboard::Dashboard;
use crate::screens::holding_detail::HoldingDetail;
use crate::screens::portfolio::Portfolio;
use crate::screens::reports::Reports;
use crate::screens::sell_form::SellForm;
use dioxus::prelude::*;

// `*Route` on every variant is Dioxus's own idiomatic Routable convention
// - not a naming problem to fix by renaming away from it.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(Shell)]
    #[route("/", Dashboard)]
    DashboardRoute {},
    #[route("/portfolio", Portfolio)]
    PortfolioRoute {},
    #[route("/ledger", LedgerPlaceholder)]
    LedgerRoute {},
    #[route("/reports", Reports)]
    ReportsRoute {},
    #[route("/settings", SettingsPlaceholder)]
    SettingsRoute {},
    #[route("/holding/:id", HoldingDetail)]
    HoldingDetailRoute { id: i64 },
    #[route("/buy", BuyForm)]
    BuyRoute {},
    #[route("/sell", SellForm)]
    SellRoute {},
    // Reached from a specific holding's own detail page - pre-selects that
    // holding in `SellForm` instead of dropping into a bare picker that
    // would force re-searching for the same card. Same reasoning as
    // `CompForHoldingRoute` below.
    #[route("/sell/:holding_id", SellFormForHolding)]
    SellForHoldingRoute { holding_id: i64 },
    #[route("/comp", CompForm)]
    CompRoute {},
    // Reached from a specific holding's own detail page - pre-selects that
    // holding in `CompForm` instead of dropping into a bare picker that
    // would force re-searching for the same card.
    #[route("/comp/:holding_id", CompFormForHolding)]
    CompForHoldingRoute { holding_id: i64 },
}

#[component]
fn SellFormForHolding(holding_id: i64) -> Element {
    rsx! {
        SellForm { holding_id: Some(holding_id) }
    }
}

#[component]
fn CompFormForHolding(holding_id: i64) -> Element {
    rsx! {
        CompForm { holding_id: Some(holding_id) }
    }
}

#[component]
fn LedgerPlaceholder() -> Element {
    rsx! {
        Placeholder { title: "Ledger" }
    }
}

#[component]
fn SettingsPlaceholder() -> Element {
    rsx! {
        Placeholder { title: "Settings" }
    }
}

#[component]
fn Placeholder(title: String) -> Element {
    rsx! {
        div { class: "p-8",
            h1 { class: "text-2xl font-semibold m-0", "{title}" }
            p { class: "text-text-secondary mt-2", "Not built yet." }
        }
    }
}
