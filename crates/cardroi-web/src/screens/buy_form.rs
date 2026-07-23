//! The Buy form: records an acquisition, creating a new holding. Two
//! card-selection modes:
//!
//! - **Existing card** (search + pick): calls `Repository::
//!   record_acquisition` directly - the GUI analog of `cardroi buy
//!   --card-id <id>`.
//! - **New card** (type set + card details in): calls `Repository::
//!   import_acquisitions` with a single row - the GUI analog of `cardroi
//!   import`'s find-or-create semantics.
//!
//! Both paths call the exact same `NewHolding`/`NewTransaction`/`NewSet`/
//! `NewCard::validate()` the CLI already uses - no parallel validation.

use cardroi::db::repository::{AcquisitionImportRow, Repository};
use cardroi::error::Result as CardRoiResult;
use cardroi::models::{
    HoldingImage, Money, NewCard, NewHolding, NewSet, NewTransaction, TransactionType,
};
use chrono::NaiveDate;
use dioxus::prelude::*;

use crate::components::form_field::FormField;
use crate::components::photo::{PhotoCapture, PhotoGallery};
use crate::routes::Route;
use crate::screens::format::money;
use crate::web_bridge::WebBridge;

#[derive(Clone, Debug, PartialEq)]
struct CardOption {
    card_id: i64,
    label: String,
}

fn load_card_options(repo: &Repository) -> CardRoiResult<Vec<CardOption>> {
    let cards = repo.list_cards(None)?;
    let mut options = Vec::with_capacity(cards.len());
    for card in &cards {
        let set = repo.get_set(card.set_id)?;
        options.push(CardOption {
            card_id: card.id,
            label: format!("{} - {}", set.name, card.display_name()),
        });
    }
    options.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(options)
}

#[component]
fn CardPicker(selected: Signal<Option<CardOption>>) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut query = use_signal(String::new);
    let options = use_resource(move || {
        let bridge = bridge.clone();
        async move { bridge.run(load_card_options).await }
    });

    if let Some(chosen) = selected() {
        return rsx! {
            div { class: "flex items-center gap-2",
                span { "{chosen.label}" }
                button {
                    class: "text-gold text-sm bg-transparent border-none cursor-pointer p-0",
                    onclick: move |_| selected.set(None),
                    "change"
                }
            }
        };
    }

    match &*options.read() {
        None => rsx! { span { class: "text-text-secondary text-sm", "Loading cards..." } },
        Some(Err(err)) => rsx! { span { class: "text-loss text-sm", "{err}" } },
        Some(Ok(all_options)) => {
            let query_lower = query().to_lowercase();
            let filtered: Vec<CardOption> = all_options
                .iter()
                .filter(|o| query_lower.is_empty() || o.label.to_lowercase().contains(&query_lower))
                .cloned()
                .collect();
            rsx! {
                div {
                    input {
                        class: "bg-surface text-text-primary border border-border rounded-radius px-2 py-1.5 font-data w-full",
                        placeholder: "Search cards...",
                        value: "{query}",
                        oninput: move |evt| query.set(evt.value()),
                    }
                    if filtered.is_empty() {
                        p { class: "text-text-secondary text-sm mt-2", "No matching cards." }
                    } else {
                        div { class: "flex flex-col mt-2", style: "max-height: 240px; overflow-y: auto;",
                            for option in filtered {
                                button {
                                    class: "text-left px-2 py-1.5 bg-transparent border-none cursor-pointer hover:bg-surface rounded-radius",
                                    onclick: {
                                        let option = option.clone();
                                        move |_| selected.set(Some(option.clone()))
                                    },
                                    "{option.label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardMode {
    Existing,
    New,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AcquisitionInputs {
    pub(crate) price: String,
    pub(crate) fees: String,
    pub(crate) shipping: String,
    pub(crate) tax: String,
    pub(crate) other_cost: String,
    pub(crate) date: String,
    pub(crate) serial: String,
    pub(crate) grade: String,
    pub(crate) grading_company: String,
    pub(crate) cert: String,
    pub(crate) counterparty: String,
    pub(crate) platform: String,
    pub(crate) external_ref: String,
    pub(crate) notes: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NewCardInputs {
    pub(crate) set_name: String,
    pub(crate) sport: String,
    pub(crate) year: String,
    pub(crate) brand: String,
    pub(crate) card_number: String,
    pub(crate) player_name: String,
    pub(crate) variant: String,
    pub(crate) parallel_name: String,
    pub(crate) print_run: String,
    pub(crate) is_rookie: bool,
    pub(crate) is_autograph: bool,
    pub(crate) is_relic: bool,
}

fn parse_money_or_zero(s: &str) -> CardRoiResult<Money> {
    use std::str::FromStr;
    if s.trim().is_empty() {
        Ok(Money::ZERO)
    } else {
        Money::from_str(s)
    }
}

fn parse_date_or_today(s: &str) -> CardRoiResult<NaiveDate> {
    use cardroi::error::CardRoiError;
    if s.trim().is_empty() {
        Ok(chrono::Utc::now().date_naive())
    } else {
        crate::screens::format::parse_date(s).map_err(CardRoiError::validation)
    }
}

fn build_transaction(holding_id: i64, inputs: &AcquisitionInputs) -> CardRoiResult<NewTransaction> {
    let price = parse_money_or_zero(&inputs.price)?;
    let fees = parse_money_or_zero(&inputs.fees)?;
    let shipping = parse_money_or_zero(&inputs.shipping)?;
    let tax = parse_money_or_zero(&inputs.tax)?;
    let other_cost = parse_money_or_zero(&inputs.other_cost)?;
    let transaction_date = parse_date_or_today(&inputs.date)?;

    Ok(NewTransaction {
        holding_id,
        transaction_type: TransactionType::Acquisition,
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

fn build_holding(card_id: i64, acquired_date: NaiveDate, inputs: &AcquisitionInputs) -> NewHolding {
    NewHolding {
        card_id,
        serial_number: (!inputs.serial.trim().is_empty()).then(|| inputs.serial.clone()),
        grade: (!inputs.grade.trim().is_empty()).then(|| inputs.grade.clone()),
        grading_company: (!inputs.grading_company.trim().is_empty())
            .then(|| inputs.grading_company.clone()),
        cert_number: (!inputs.cert.trim().is_empty()).then(|| inputs.cert.clone()),
        acquired_date: Some(acquired_date),
        notes: (!inputs.notes.trim().is_empty()).then(|| inputs.notes.clone()),
    }
}

fn submit_existing_card(
    card_id: i64,
    inputs: AcquisitionInputs,
    repo: &Repository,
) -> CardRoiResult<(i64, Money)> {
    let transaction_date = parse_date_or_today(&inputs.date)?;
    let holding = build_holding(card_id, transaction_date, &inputs);
    let new_txn = build_transaction(0, &inputs)?;
    let (holding, txn) = repo.record_acquisition(&holding, new_txn)?;
    Ok((holding.id, txn.total))
}

pub(crate) fn submit_new_card(
    card_inputs: NewCardInputs,
    acquisition_inputs: AcquisitionInputs,
    repo: &Repository,
) -> CardRoiResult<Money> {
    use cardroi::error::CardRoiError;

    let year = crate::screens::format::parse_optional_i32(&card_inputs.year)
        .map_err(CardRoiError::validation)?;
    let print_run = crate::screens::format::parse_optional_i32(&card_inputs.print_run)
        .map_err(CardRoiError::validation)?;
    if card_inputs.set_name.trim().is_empty() {
        return Err(CardRoiError::validation("set name must not be empty"));
    }
    if card_inputs.sport.trim().is_empty() {
        return Err(CardRoiError::validation("set sport must not be empty"));
    }

    let set = NewSet {
        name: card_inputs.set_name,
        sport: card_inputs.sport,
        year,
        brand: (!card_inputs.brand.trim().is_empty()).then_some(card_inputs.brand),
        total_cards: None,
        notes: None,
    };
    let card = NewCard {
        set_id: 0,
        card_number: card_inputs.card_number,
        player_name: card_inputs.player_name,
        variant: (!card_inputs.variant.trim().is_empty()).then_some(card_inputs.variant),
        parallel_name: (!card_inputs.parallel_name.trim().is_empty())
            .then_some(card_inputs.parallel_name),
        print_run,
        is_rookie: card_inputs.is_rookie,
        is_autograph: card_inputs.is_autograph,
        is_relic: card_inputs.is_relic,
        notes: None,
    };
    let transaction_date = parse_date_or_today(&acquisition_inputs.date)?;
    let holding = build_holding(0, transaction_date, &acquisition_inputs);
    let transaction = build_transaction(0, &acquisition_inputs)?;

    let summary = repo.import_acquisitions(&[AcquisitionImportRow {
        set,
        card,
        holding,
        transaction,
    }])?;
    debug_assert_eq!(summary.rows_imported, 1);

    // import_acquisitions doesn't hand back the created transaction, so
    // recompute the same total() the backend just inserted - no parallel
    // math, just NewTransaction::total() again on the same inputs.
    build_transaction(0, &acquisition_inputs).map(|t| t.total())
}

#[component]
pub fn BuyForm() -> Element {
    let bridge = use_context::<WebBridge>();
    let mut mode = use_signal(|| CardMode::Existing);
    let selected_card = use_signal(|| None::<CardOption>);

    let set_name_input = use_signal(String::new);
    let sport_input = use_signal(String::new);
    let year_input = use_signal(String::new);
    let brand_input = use_signal(String::new);
    let card_number_input = use_signal(String::new);
    let player_name_input = use_signal(String::new);
    let variant_input = use_signal(String::new);
    let parallel_input = use_signal(String::new);
    let print_run_input = use_signal(String::new);
    let mut is_rookie = use_signal(|| false);
    let mut is_autograph = use_signal(|| false);
    let mut is_relic = use_signal(|| false);

    let price_input = use_signal(String::new);
    let fees_input = use_signal(String::new);
    let shipping_input = use_signal(String::new);
    let tax_input = use_signal(String::new);
    let other_cost_input = use_signal(String::new);
    let date_input = use_signal(String::new);
    let serial_input = use_signal(String::new);
    let grade_input = use_signal(String::new);
    let grading_company_input = use_signal(String::new);
    let cert_input = use_signal(String::new);
    let counterparty_input = use_signal(String::new);
    let platform_input = use_signal(String::new);
    let external_ref_input = use_signal(String::new);
    let notes_input = use_signal(String::new);

    let mut error = use_signal(|| None::<String>);
    let mut submitted_total = use_signal(|| None::<Money>);
    // Only ever `Some` on the "existing card" path - `import_acquisitions`
    // (the "new card" path) hands back no per-row created holding id at
    // all, since it's a bulk-import-shaped API shared with the CLI's
    // `cardroi import`. A card bought via "new card" still gets its photo
    // added afterward on Card Details, which already fully supports it.
    let mut submitted_holding_id = use_signal(|| None::<i64>);
    // What PhotoCapture/PhotoGallery actually display on the success
    // screen - reloaded from the real repository after every upload/
    // delete/reorder rather than hand-maintained locally, so this view
    // can never drift from what's actually stored.
    let mut bought_photos = use_signal(Vec::<HoldingImage>::new);
    let reload_bought_photos = {
        let bridge = bridge.clone();
        move || {
            let bridge = bridge.clone();
            spawn(async move {
                if let Some(holding_id) = submitted_holding_id() {
                    let photos = bridge
                        .run(move |repo| repo.list_photos_for_holding(holding_id))
                        .await
                        .unwrap_or_default();
                    bought_photos.set(photos);
                }
            });
        }
    };

    let acquisition_inputs = move || AcquisitionInputs {
        price: price_input(),
        fees: fees_input(),
        shipping: shipping_input(),
        tax: tax_input(),
        other_cost: other_cost_input(),
        date: date_input(),
        serial: serial_input(),
        grade: grade_input(),
        grading_company: grading_company_input(),
        cert: cert_input(),
        counterparty: counterparty_input(),
        platform: platform_input(),
        external_ref: external_ref_input(),
        notes: notes_input(),
    };

    let running_total = use_memo(move || {
        build_transaction(0, &acquisition_inputs())
            .ok()
            .map(|t| t.total())
    });

    let submit = move |_| {
        let bridge = bridge.clone();
        let inputs = acquisition_inputs();
        match mode() {
            CardMode::Existing => {
                let Some(card) = selected_card() else {
                    error.set(Some("choose a card first".to_string()));
                    return;
                };
                spawn(async move {
                    let outcome = bridge
                        .run(move |repo| submit_existing_card(card.card_id, inputs, repo))
                        .await;
                    match outcome {
                        Ok((holding_id, total)) => {
                            error.set(None);
                            submitted_holding_id.set(Some(holding_id));
                            submitted_total.set(Some(total));
                        }
                        Err(err) => error.set(Some(err.to_string())),
                    }
                });
            }
            CardMode::New => {
                let card_inputs = NewCardInputs {
                    set_name: set_name_input(),
                    sport: sport_input(),
                    year: year_input(),
                    brand: brand_input(),
                    card_number: card_number_input(),
                    player_name: player_name_input(),
                    variant: variant_input(),
                    parallel_name: parallel_input(),
                    print_run: print_run_input(),
                    is_rookie: is_rookie(),
                    is_autograph: is_autograph(),
                    is_relic: is_relic(),
                };
                spawn(async move {
                    let outcome = bridge
                        .run(move |repo| submit_new_card(card_inputs, inputs, repo))
                        .await;
                    match outcome {
                        Ok(total) => {
                            error.set(None);
                            submitted_total.set(Some(total));
                        }
                        Err(err) => error.set(Some(err.to_string())),
                    }
                });
            }
        }
    };

    if let Some(total) = submitted_total() {
        return rsx! {
            div { class: "p-8 flex flex-col gap-4 max-w-2xl",
                h1 { class: "text-2xl font-semibold m-0", "Log buy" }
                p { class: "text-gain m-0", "Bought - total cost basis {money(total)}." }
                // Only ever shown on the "existing card" path - see
                // `submitted_holding_id`'s own doc comment for why "new
                // card" doesn't get this yet. Optional and skippable by
                // simply not interacting with it, same as everything
                // else on this page - never a blocking step in the
                // celebratory "just bought this" moment.
                if let Some(holding_id) = submitted_holding_id() {
                    div { class: "flex flex-col gap-3 mt-2",
                        if !bought_photos().is_empty() {
                            PhotoGallery {
                                key: "{holding_id}",
                                holding_id,
                                photos: bought_photos(),
                                on_changed: {
                                    let reload_bought_photos = reload_bought_photos.clone();
                                    move |_| reload_bought_photos()
                                },
                            }
                        }
                        PhotoCapture {
                            key: "{holding_id}",
                            holding_id,
                            on_uploaded: {
                                let reload_bought_photos = reload_bought_photos.clone();
                                move |_| reload_bought_photos()
                            },
                        }
                        Link {
                            to: Route::HoldingDetailRoute { id: holding_id },
                            class: "text-gold text-sm no-underline",
                            "View card"
                        }
                    }
                }
            }
        };
    }

    rsx! {
        div { class: "p-8 flex flex-col gap-4 max-w-2xl",
            h1 { class: "text-2xl font-semibold m-0", "Log buy" }

            div { class: "flex gap-2",
                button {
                    class: if mode() == CardMode::Existing { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| mode.set(CardMode::Existing),
                    "Existing card"
                }
                button {
                    class: if mode() == CardMode::New { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                    onclick: move |_| mode.set(CardMode::New),
                    "New card"
                }
            }

            if mode() == CardMode::Existing {
                div {
                    label { class: "text-text-secondary text-xs", "Card" }
                    CardPicker { selected: selected_card }
                }
            } else {
                div { class: "grid grid-cols-1 sm:grid-cols-2 gap-3",
                    FormField { label: "Set name", value: set_name_input, placeholder: "e.g. 2023 Topps Chrome" }
                    FormField { label: "Sport", value: sport_input, placeholder: "e.g. Basketball" }
                    FormField { label: "Year", value: year_input, placeholder: "e.g. 2023" }
                    FormField { label: "Brand", value: brand_input, placeholder: "e.g. Topps" }
                    FormField { label: "Card number", value: card_number_input, placeholder: "e.g. 123" }
                    FormField { label: "Player name", value: player_name_input, placeholder: "" }
                    FormField { label: "Variant", value: variant_input, placeholder: "" }
                    FormField { label: "Parallel", value: parallel_input, placeholder: "" }
                    FormField { label: "Print run", value: print_run_input, placeholder: "" }
                }
                div { class: "flex gap-4",
                    label { class: "flex items-center gap-2 text-sm",
                        input { r#type: "checkbox", checked: is_rookie(), onchange: move |evt| is_rookie.set(evt.checked()) }
                        "Rookie"
                    }
                    label { class: "flex items-center gap-2 text-sm",
                        input { r#type: "checkbox", checked: is_autograph(), onchange: move |evt| is_autograph.set(evt.checked()) }
                        "Autograph"
                    }
                    label { class: "flex items-center gap-2 text-sm",
                        input { r#type: "checkbox", checked: is_relic(), onchange: move |evt| is_relic.set(evt.checked()) }
                        "Relic"
                    }
                }
            }

            div { class: "grid grid-cols-2 sm:grid-cols-3 gap-3",
                FormField { label: "Price", value: price_input, placeholder: "0.00" }
                FormField { label: "Fees", value: fees_input, placeholder: "0.00" }
                FormField { label: "Shipping", value: shipping_input, placeholder: "0.00" }
                FormField { label: "Tax", value: tax_input, placeholder: "0.00" }
                FormField { label: "Other cost", value: other_cost_input, placeholder: "0.00" }
                FormField { label: "Date", value: date_input, placeholder: "MM-DD-YYYY (today)" }
                FormField { label: "Serial", value: serial_input, placeholder: "e.g. 12/25" }
                FormField { label: "Grade", value: grade_input, placeholder: "" }
                FormField { label: "Grading company", value: grading_company_input, placeholder: "" }
                FormField { label: "Cert number", value: cert_input, placeholder: "" }
                FormField { label: "Counterparty", value: counterparty_input, placeholder: "" }
                FormField { label: "Platform", value: platform_input, placeholder: "" }
                FormField { label: "External ref", value: external_ref_input, placeholder: "" }
            }
            FormField { label: "Notes", value: notes_input, placeholder: "" }

            div { class: "flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3",
                span { class: "text-text-secondary text-sm",
                    "Total cost basis: "
                    if let Some(total) = running_total() {
                        span { class: "data-numeral text-text-primary", "{money(total)}" }
                    } else {
                        span { "-" }
                    }
                }
                button {
                    class: "px-4 py-2 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer",
                    onclick: submit,
                    "Buy"
                }
            }

            if let Some(err) = error() {
                p { class: "text-loss m-0", "{err}" }
            }
        }
    }
}
