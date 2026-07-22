//! The Sell form: records a disposition against an existing,
//! currently-owned holding - the GUI analog of `cardroi sell`. Calls
//! `Repository::record_sale` directly with the exact same `NewTransaction`
//! the CLI builds; no parallel validation.

use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{Money, NewTransaction, Transaction, TransactionType};
use dioxus::prelude::*;

use crate::components::form_field::FormField;
use crate::components::holding_picker::{HoldingOption, HoldingPicker, load_single_holding_option};
use crate::screens::format::money;
use crate::web_bridge::WebBridge;

#[derive(Clone, Debug, Default)]
pub(crate) struct SellInputs {
    pub(crate) price: String,
    pub(crate) fees: String,
    pub(crate) shipping: String,
    pub(crate) tax: String,
    pub(crate) other_cost: String,
    pub(crate) date: String,
    pub(crate) counterparty: String,
    pub(crate) platform: String,
    pub(crate) external_ref: String,
    pub(crate) notes: String,
}

fn parse_money_or_zero(s: &str) -> CardRoiResult<Money> {
    use std::str::FromStr;
    if s.trim().is_empty() {
        Ok(Money::ZERO)
    } else {
        Money::from_str(s)
    }
}

fn build_transaction(holding_id: i64, inputs: &SellInputs) -> CardRoiResult<NewTransaction> {
    use cardroi::error::CardRoiError;

    let price = parse_money_or_zero(&inputs.price)?;
    let fees = parse_money_or_zero(&inputs.fees)?;
    let shipping = parse_money_or_zero(&inputs.shipping)?;
    let tax = parse_money_or_zero(&inputs.tax)?;
    let other_cost = parse_money_or_zero(&inputs.other_cost)?;
    let transaction_date = if inputs.date.trim().is_empty() {
        chrono::Utc::now().date_naive()
    } else {
        crate::screens::format::parse_date(&inputs.date).map_err(CardRoiError::validation)?
    };

    Ok(NewTransaction {
        holding_id,
        transaction_type: TransactionType::Disposition,
        transaction_date,
        price,
        fees,
        shipping,
        tax,
        other_cost,
        counterparty: (!inputs.counterparty.trim().is_empty()).then(|| inputs.counterparty.clone()),
        platform: (!inputs.platform.trim().is_empty()).then(|| inputs.platform.clone()),
        external_ref: (!inputs.external_ref.trim().is_empty()).then(|| inputs.external_ref.clone()),
        notes: (!inputs.notes.trim().is_empty()).then(|| inputs.notes.clone()),
        ..Default::default()
    })
}

pub(crate) fn submit_sale(
    holding_id: i64,
    inputs: SellInputs,
    repo: &Repository,
) -> CardRoiResult<Transaction> {
    let new_txn = build_transaction(holding_id, &inputs)?;
    repo.record_sale(new_txn)
}

#[component]
pub fn SellForm(#[props(default)] holding_id: Option<i64>) -> Element {
    use cardroi::models::HoldingStatus;

    let bridge = use_context::<WebBridge>();
    let mut selected = use_signal(|| None::<HoldingOption>);

    // Reached from a specific holding's own detail page - that holding is
    // already in view, so pre-select it instead of reopening the same
    // "which card?" search the collector just came from. Same pattern as
    // `CompForm`'s identical pre-select block.
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

    let price_input = use_signal(String::new);
    let fees_input = use_signal(String::new);
    let shipping_input = use_signal(String::new);
    let tax_input = use_signal(String::new);
    let other_cost_input = use_signal(String::new);
    let date_input = use_signal(String::new);
    let counterparty_input = use_signal(String::new);
    let platform_input = use_signal(String::new);
    let external_ref_input = use_signal(String::new);
    let notes_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut submitted_total = use_signal(|| None::<Money>);

    let running_total = use_memo(move || {
        let inputs = SellInputs {
            price: price_input(),
            fees: fees_input(),
            shipping: shipping_input(),
            tax: tax_input(),
            other_cost: other_cost_input(),
            date: date_input(),
            counterparty: String::new(),
            platform: String::new(),
            external_ref: String::new(),
            notes: String::new(),
        };
        build_transaction(0, &inputs).ok().map(|t| t.total())
    });

    let submit = move |_| {
        let Some(holding) = selected() else {
            error.set(Some("choose a holding first".to_string()));
            return;
        };
        let bridge = bridge.clone();
        let inputs = SellInputs {
            price: price_input(),
            fees: fees_input(),
            shipping: shipping_input(),
            tax: tax_input(),
            other_cost: other_cost_input(),
            date: date_input(),
            counterparty: counterparty_input(),
            platform: platform_input(),
            external_ref: external_ref_input(),
            notes: notes_input(),
        };
        spawn(async move {
            let outcome = bridge
                .run(move |repo| submit_sale(holding.holding_id, inputs, repo))
                .await;
            match outcome {
                Ok(txn) => {
                    error.set(None);
                    submitted_total.set(Some(txn.total));
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    if let Some(total) = submitted_total() {
        return rsx! {
            div { class: "p-8 max-w-2xl",
                h1 { class: "text-2xl font-semibold m-0 mb-4", "Log sell" }
                p { class: "text-gain m-0", "Sold - net proceeds {money(total)}." }
            }
        };
    }

    rsx! {
        div { class: "p-8 flex flex-col gap-4 max-w-2xl",
            h1 { class: "text-2xl font-semibold m-0", "Log sell" }

            div {
                label { class: "text-text-secondary text-xs", "Holding" }
                HoldingPicker { selected, status_filter: Some(HoldingStatus::Owned) }
            }

            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                FormField { label: "Price", value: price_input, placeholder: "0.00" }
                FormField { label: "Fees", value: fees_input, placeholder: "0.00" }
                FormField { label: "Shipping", value: shipping_input, placeholder: "0.00" }
                FormField { label: "Tax", value: tax_input, placeholder: "0.00" }
                FormField { label: "Other cost", value: other_cost_input, placeholder: "0.00" }
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY (today)" }
                FormField { label: "Counterparty", value: counterparty_input, placeholder: "" }
                FormField { label: "Platform", value: platform_input, placeholder: "" }
                FormField { label: "External ref", value: external_ref_input, placeholder: "" }
            }
            FormField { label: "Notes", value: notes_input, placeholder: "" }

            div { class: "flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3",
                span { class: "text-text-secondary text-sm",
                    "Net proceeds: "
                    if let Some(total) = running_total() {
                        span { class: "data-numeral text-text-primary", "{money(total)}" }
                    } else {
                        span { "-" }
                    }
                }
                button {
                    class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer",
                    onclick: submit,
                    "Sell"
                }
            }

            if let Some(err) = error() {
                p { class: "text-loss m-0", "{err}" }
            }
        }
    }
}
