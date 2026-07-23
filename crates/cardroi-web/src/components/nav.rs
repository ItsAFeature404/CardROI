//! The persistent app chrome: a responsive nav, rendered once as the
//! `#[layout(Shell)]` wrapping every route in `routes::Route` - only the
//! `Outlet` content below it changes on navigation, the nav itself never
//! re-mounts. Two physical `<nav>` elements, not one nav reflowed via CSS:
//! a sidebar (`hidden md:flex`) at desktop widths, and a bottom bar
//! (`flex md:hidden`) at phone widths - a shrunk-to-fit sidebar reads
//! wrong on a phone, and a bottom bar is the standard, thumb-reachable
//! mobile nav pattern this item count (5 destinations) fits well.
//!
//! Quick actions (Log Buy/Log Sell) are pinned at the bottom of the
//! desktop sidebar. On phone width they're reachable via
//! `MobileQuickActions`, a floating action button anchored above the
//! bottom bar instead - the bottom bar's 5 destinations plus 2 actions is
//! past where a bottom bar stays usable as tappable tabs, so the actions
//! get their own affordance rather than competing for the same row.
//!
//! The desktop sidebar carries this app's one wordmark, in `font-brand` -
//! before this, nothing in the running app said "CardROI" anywhere (the
//! browser tab didn't either, see `main.rs`'s `document::Title`).
//! Deliberately not repeated on the mobile bottom bar too: a persistent
//! top bar would cost real vertical space on a phone screen for a fix
//! that already reaches every screen once, here.

use crate::routes::Route;
use dioxus::prelude::*;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::ld_icons::{
    LdBriefcase, LdFileText, LdLayoutDashboard, LdPieChart, LdPlus, LdSettings, LdX,
};

#[component]
pub fn Shell() -> Element {
    rsx! {
        div {
            class: "flex min-h-screen bg-canvas text-text-primary font-data",

            nav {
                class: "hidden md:flex w-56 shrink-0 bg-surface border-r border-border flex-col p-4 gap-1",
                p { class: "font-brand text-xl text-text-primary px-3 mb-3", "CardROI" }
                SidebarLink { to: Route::DashboardRoute {}, label: "Dashboard",
                    Icon { icon: LdLayoutDashboard, width: 20, height: 20 }
                }
                SidebarLink { to: Route::PortfolioRoute {}, label: "Portfolio",
                    Icon { icon: LdBriefcase, width: 20, height: 20 }
                }
                SidebarLink { to: Route::LedgerRoute {}, label: "Ledger",
                    Icon { icon: LdFileText, width: 20, height: 20 }
                }
                SidebarLink { to: Route::ReportsRoute {}, label: "Reports",
                    Icon { icon: LdPieChart, width: 20, height: 20 }
                }
                SidebarLink { to: Route::SettingsRoute {}, label: "Settings",
                    Icon { icon: LdSettings, width: 20, height: 20 }
                }

                div {
                    class: "mt-auto flex flex-col gap-2 pt-4 border-t border-border",
                    QuickAction { to: Route::BuyRoute {}, label: "Log Buy" }
                    QuickAction { to: Route::SellRoute {}, label: "Log Sell" }
                }
            }

            main { class: "flex-1 overflow-auto pb-16 md:pb-0", Outlet::<Route> {} }

            MobileQuickActions {}

            nav {
                class: "flex md:hidden fixed bottom-0 left-0 right-0 bg-surface border-t border-border justify-around items-stretch",
                BottomLink { to: Route::DashboardRoute {}, label: "Dashboard",
                    Icon { icon: LdLayoutDashboard, width: 20, height: 20 }
                }
                BottomLink { to: Route::PortfolioRoute {}, label: "Portfolio",
                    Icon { icon: LdBriefcase, width: 20, height: 20 }
                }
                BottomLink { to: Route::LedgerRoute {}, label: "Ledger",
                    Icon { icon: LdFileText, width: 20, height: 20 }
                }
                BottomLink { to: Route::ReportsRoute {}, label: "Reports",
                    Icon { icon: LdPieChart, width: 20, height: 20 }
                }
                BottomLink { to: Route::SettingsRoute {}, label: "Settings",
                    Icon { icon: LdSettings, width: 20, height: 20 }
                }
            }
        }
    }
}

#[component]
fn SidebarLink(to: Route, label: String, children: Element) -> Element {
    rsx! {
        Link {
            to,
            active_class: "bg-surface-elevated text-gold",
            class: "flex items-center gap-3 px-3 py-2 rounded-radius text-text-secondary no-underline transition-colors duration-[var(--duration-standard)] ease-standard hover:bg-surface-elevated hover:text-text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold focus-visible:outline-offset-[-2px]",
            {children}
            span { "{label}" }
        }
    }
}

#[component]
fn QuickAction(to: Route, label: String) -> Element {
    rsx! {
        Link {
            to,
            class: "flex items-center justify-center gap-2 px-3 py-2 rounded-radius bg-gold text-canvas border-none no-underline font-semibold cursor-pointer transition-colors duration-[var(--duration-standard)] ease-standard focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold focus-visible:outline-offset-2",
            Icon { icon: LdPlus, width: 16, height: 16 }
            span { "{label}" }
        }
    }
}

/// Phone-only floating action button, anchored above the bottom bar
/// (`bottom-20` clears its height). Tapping it toggles a small menu with
/// the same Log Buy/Log Sell destinations the desktop sidebar pins -
/// Comp is deliberately not here: it's an occasional per-holding action
/// reached from that holding's own detail page, not a high-frequency one
/// that needs a shortcut.
#[component]
fn MobileQuickActions() -> Element {
    let mut open = use_signal(|| false);

    rsx! {
        div {
            class: "flex md:hidden fixed bottom-20 right-4 z-10 flex-col items-end gap-2",
            if open() {
                div {
                    class: "flex flex-col gap-2",
                    Link {
                        to: Route::SellRoute {},
                        class: "px-4 py-2 rounded-radius bg-surface-elevated text-text-primary border border-border no-underline font-semibold shadow-lg",
                        onclick: move |_| open.set(false),
                        "Log Sell"
                    }
                    Link {
                        to: Route::BuyRoute {},
                        class: "px-4 py-2 rounded-radius bg-surface-elevated text-text-primary border border-border no-underline font-semibold shadow-lg",
                        onclick: move |_| open.set(false),
                        "Log Buy"
                    }
                }
            }
            button {
                class: "flex items-center justify-center w-14 h-14 rounded-full bg-gold text-canvas border-none cursor-pointer shadow-lg focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold focus-visible:outline-offset-2",
                "aria-label": if open() { "Close quick actions" } else { "Open quick actions" },
                onclick: move |_| open.set(!open()),
                if open() {
                    Icon { icon: LdX, width: 24, height: 24 }
                } else {
                    Icon { icon: LdPlus, width: 24, height: 24 }
                }
            }
        }
    }
}

#[component]
fn BottomLink(to: Route, label: String, children: Element) -> Element {
    rsx! {
        Link {
            to,
            active_class: "text-gold",
            class: "flex flex-col items-center justify-center gap-1 flex-1 py-2 text-text-secondary no-underline text-xs transition-colors duration-[var(--duration-standard)] ease-standard focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold focus-visible:outline-offset-[-2px]",
            {children}
            span { "{label}" }
        }
    }
}
