//! The Comp form: records a comp - a comparable sold listing's price, as
//! the hobby actually prices cards - for an existing holding, the GUI
//! analog of `cardroi comp add`. Calls `Repository::create_appraisal`
//! directly with the exact same `NewAppraisal` the CLI builds (the model
//! and schema keep their original internal names; only the vocabulary a
//! user actually reads is "comp" rather than "appraisal"); no parallel
//! validation.

use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{Appraisal, Money, NewAppraisal};
use dioxus::prelude::*;

use crate::components::form_field::FormField;
use crate::components::holding_picker::{HoldingOption, HoldingPicker, load_single_holding_option};
use crate::web_bridge::WebBridge;

#[derive(Clone, Debug, Default)]
pub(crate) struct CompInputs {
    pub(crate) value: String,
    pub(crate) date: String,
    pub(crate) source: String,
    pub(crate) notes: String,
}

pub(crate) fn submit_comp(
    holding_id: i64,
    inputs: CompInputs,
    repo: &Repository,
) -> CardRoiResult<Appraisal> {
    use cardroi::error::CardRoiError;
    use std::str::FromStr;

    let appraised_value = Money::from_str(&inputs.value)?;
    let appraised_date = if inputs.date.trim().is_empty() {
        chrono::Utc::now().date_naive()
    } else {
        crate::screens::format::parse_date(&inputs.date).map_err(CardRoiError::validation)?
    };
    let source = (!inputs.source.trim().is_empty()).then_some(inputs.source);
    let notes = (!inputs.notes.trim().is_empty()).then_some(inputs.notes);

    repo.create_appraisal(&NewAppraisal {
        holding_id,
        appraised_value,
        appraised_date,
        source,
        notes,
    })
}

#[component]
pub fn CompForm(#[props(default)] holding_id: Option<i64>) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut selected = use_signal(|| None::<HoldingOption>);
    let value_input = use_signal(String::new);
    let date_input = use_signal(String::new);
    let source_input = use_signal(String::new);
    let notes_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut submitted = use_signal(|| false);

    // Reached from a specific holding's detail page ("+ Add comp") - that
    // holding is already in view, so pre-select it instead of reopening
    // the same "which card?" search the user just came from.
    use_effect({
        let bridge = bridge.clone();
        move || {
            let Some(id) = holding_id else { return };
            if selected.peek().is_some() {
                return;
            }
            let bridge = bridge.clone();
            spawn(async move {
                if let Ok(option) = bridge
                    .run(move |repo| load_single_holding_option(id, repo))
                    .await
                {
                    selected.set(Some(option));
                }
            });
        }
    });

    let submit = move |_| {
        let Some(holding) = selected() else {
            error.set(Some("choose a holding first".to_string()));
            return;
        };
        let bridge = bridge.clone();
        let inputs = CompInputs {
            value: value_input(),
            date: date_input(),
            source: source_input(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_comp(holding.holding_id, inputs, repo))
                .await;
            match outcome {
                Ok(_) => {
                    error.set(None);
                    submitted.set(true);
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    if submitted() {
        return rsx! {
            div { class: "p-8 max-w-2xl",
                h1 { class: "text-2xl font-semibold m-0 mb-4", "Add comp" }
                p { class: "text-gain m-0", "Recorded." }
            }
        };
    }

    rsx! {
        div { class: "p-8 flex flex-col gap-4 max-w-2xl",
            h1 { class: "text-2xl font-semibold m-0", "Add comp" }

            div {
                label { class: "text-text-secondary text-xs", "Holding" }
                HoldingPicker { selected, status_filter: None }
            }

            div { class: "grid grid-cols-1 sm:grid-cols-2 gap-3",
                FormField { label: "Value", value: value_input, placeholder: "0.00" }
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY (today)" }
                FormField { label: "Source", value: source_input, placeholder: "e.g. eBay comps" }
                FormField { label: "Notes", value: notes_input, placeholder: "" }
            }

            button {
                class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer self-start",
                onclick: submit,
                "Record"
            }

            if let Some(err) = error() {
                p { class: "text-loss m-0", "{err}" }
            }
        }
    }
}
