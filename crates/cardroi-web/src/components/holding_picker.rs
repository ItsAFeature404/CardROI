//! A searchable holding picker, shared by the Sell and Comp forms - both
//! need to choose an existing holding, unlike Buy which always creates a
//! new one. `status_filter` narrows the list (Sell only makes sense
//! against an `Owned` holding; Comp has no such restriction).

use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::HoldingStatus;
use dioxus::prelude::*;

use crate::web_bridge::WebBridge;

#[derive(Clone, Debug, PartialEq)]
pub struct HoldingOption {
    pub holding_id: i64,
    pub label: String,
    pub status: HoldingStatus,
}

pub(crate) fn load_holding_options(
    status_filter: Option<HoldingStatus>,
    repo: &Repository,
) -> CardRoiResult<Vec<HoldingOption>> {
    let holdings = repo.list_holdings(None, status_filter)?;
    let mut options = Vec::with_capacity(holdings.len());
    for holding in &holdings {
        let card = repo.get_card(holding.card_id)?;
        options.push(HoldingOption {
            holding_id: holding.id,
            label: format!("{} (#{})", card.display_name(), holding.id),
            status: holding.status,
        });
    }
    options.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(options)
}

/// Same shape as a single entry from `load_holding_options`, used to
/// pre-populate a picker when the caller already knows which holding it
/// wants (e.g. Comp reached from that holding's own detail page) -
/// there's no reason to make the user re-search for a card they just
/// clicked into.
pub(crate) fn load_single_holding_option(
    holding_id: i64,
    repo: &Repository,
) -> CardRoiResult<HoldingOption> {
    let holding = repo.get_holding(holding_id)?;
    let card = repo.get_card(holding.card_id)?;
    Ok(HoldingOption {
        holding_id: holding.id,
        label: format!("{} (#{})", card.display_name(), holding.id),
        status: holding.status,
    })
}

#[component]
pub fn HoldingPicker(
    selected: Signal<Option<HoldingOption>>,
    status_filter: Option<HoldingStatus>,
) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut query = use_signal(String::new);
    let options = use_resource(move || {
        let bridge = bridge.clone();
        async move {
            bridge
                .run(move |repo| load_holding_options(status_filter, repo))
                .await
        }
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
        None => rsx! { span { class: "text-text-secondary text-sm", "Loading holdings..." } },
        Some(Err(err)) => rsx! { span { class: "text-loss text-sm", "{err}" } },
        Some(Ok(all_options)) => {
            let query_lower = query().to_lowercase();
            let filtered: Vec<HoldingOption> = all_options
                .iter()
                .filter(|o| query_lower.is_empty() || o.label.to_lowercase().contains(&query_lower))
                .cloned()
                .collect();
            rsx! {
                div {
                    input {
                        class: "bg-surface text-text-primary border border-border rounded-radius px-2 py-1.5 font-data w-full",
                        placeholder: "Search holdings...",
                        value: "{query}",
                        oninput: move |evt| query.set(evt.value()),
                    }
                    if filtered.is_empty() {
                        p { class: "text-text-secondary text-sm mt-2", "No matching holdings." }
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
                                    span { class: "text-text-tertiary text-xs ml-2", "{option.status.as_str()}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
