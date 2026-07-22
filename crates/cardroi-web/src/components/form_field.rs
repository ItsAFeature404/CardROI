//! A single labeled text input, shared by every data-entry form (Buy/
//! Sell/Comp, holding detail's What-If/Mark Lost/Damaged/edit forms).
//! Deliberately just a `String` signal in and out - parsing/validation
//! happens where the real `NewTransaction`/`NewHolding`/
//! `NewAppraisal::validate()` already lives, not here.

use dioxus::prelude::*;

#[component]
pub fn FormField(label: String, value: Signal<String>, placeholder: String) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1",
            span { class: "text-text-secondary text-xs", "{label}" }
            input {
                class: "bg-surface text-text-primary border border-border rounded-radius px-2 py-1.5 font-data",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| value.set(evt.value()),
            }
        }
    }
}
