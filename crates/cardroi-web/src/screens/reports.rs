//! The "Reports" nav destination: the single Advanced-tier landing spot
//! one click deeper than the Dashboard - IRR/TWR/HHI don't belong on the
//! home screen. Tab-switches between the Performance view and the Risk/
//! Allocation view rather than adding a second top-level nav item for
//! either.

use dioxus::prelude::*;

use super::performance::Performance;
use super::risk::RiskAllocation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Performance,
    Risk,
}

#[component]
pub fn Reports() -> Element {
    let mut tab = use_signal(|| Tab::Performance);

    rsx! {
        div { class: "flex flex-col",
            div { class: "flex gap-1 px-8 pt-6 border-b border-border",
                button {
                    class: if tab() == Tab::Performance { "px-3 py-2 border-b-2 border-gold text-text-primary font-semibold bg-transparent cursor-pointer" } else { "px-3 py-2 border-b-2 border-transparent text-text-secondary bg-transparent cursor-pointer" },
                    onclick: move |_| tab.set(Tab::Performance),
                    "Performance"
                }
                button {
                    class: if tab() == Tab::Risk { "px-3 py-2 border-b-2 border-gold text-text-primary font-semibold bg-transparent cursor-pointer" } else { "px-3 py-2 border-b-2 border-transparent text-text-secondary bg-transparent cursor-pointer" },
                    onclick: move |_| tab.set(Tab::Risk),
                    "Risk & allocation"
                }
            }
            match tab() {
                Tab::Performance => rsx! { Performance {} },
                Tab::Risk => rsx! { RiskAllocation {} },
            }
        }
    }
}
