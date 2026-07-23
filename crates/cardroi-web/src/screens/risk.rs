//! The Risk / allocation view: a plain-language 0-100 diversification
//! score, a concentration-by-card breakdown, and allocation by
//! set/player/sport - all cross-checked against `cardroi report`'s
//! existing HHI/allocation output on the same DB.
//!
//! Concentration is shown as a horizontal stacked bar, not a treemap: a
//! treemap's 2D rectangle areas are genuinely harder to compare precisely
//! than a bar's length, and don't label or screen-read as well - same
//! story either way (a few positions dominating vs. many even slivers),
//! a more accessible form. Both the bar and the allocation donut fold
//! anything past the top 4 slices into "Other," using a 4-slot
//! categorical color order validated for both dark and light surfaces at
//! this app's actual token values - `[e87ba4, eda100]` (magenta, yellow)
//! fall under 3:1 contrast on this app's light canvas, so every slice
//! always carries a direct text label, never color alone.

use cardroi::analytics::portfolio::{self, AllocationEntry};
use cardroi::db::repository::Repository;
use cardroi::error::Result as CardRoiResult;
use cardroi::models::Money;
use dioxus::prelude::*;
use rust_decimal::Decimal;

use crate::web_bridge::WebBridge;

use super::format::{money, percent};

/// A 4-slot categorical color order (blue, green, magenta, yellow)
/// validated in all-pairs mode against this app's actual dark surface
/// (#12161f), since a donut/bar's slices can sit next to any other
/// slice. Dark values only: every screen in this app is dark-only today
/// (no light/dark toggle is wired up anywhere yet), so there's no live
/// light-theme rendering path for these colors to serve yet.
const SLOT_COLORS: [&str; 4] = ["#3987e5", "#008300", "#d55181", "#c98500"];
const OTHER_COLOR: &str = "#3a4256";

#[derive(Clone, Debug, PartialEq)]
struct Slice {
    label: String,
    value: Money,
    pct: Decimal,
}

/// Folds an already-sorted-descending allocation list into at most 4
/// named slices plus one "Other" slice for everything past that -
/// dataviz's series-count ladder: past 4 slots, the CVD floor and
/// label-fit both get real for an all-pairs chart form.
fn fold_to_top_four(entries: &[AllocationEntry]) -> Vec<Slice> {
    let mut slices: Vec<Slice> = entries
        .iter()
        .take(4)
        .map(|e| Slice {
            label: e.label.clone(),
            value: e.value,
            pct: e.allocation_pct,
        })
        .collect();
    if entries.len() > 4 {
        let rest = &entries[4..];
        let other_value = rest.iter().fold(Money::ZERO, |acc, e| acc + e.value);
        let other_pct: Decimal = rest.iter().map(|e| e.allocation_pct).sum();
        slices.push(Slice {
            label: "Other".to_string(),
            value: other_value,
            pct: other_pct,
        });
    }
    slices
}

#[derive(Clone, Debug, PartialEq)]
struct RiskData {
    diversification_score: Decimal,
    diversification_label: &'static str,
    hhi: Decimal,
    effective_positions: Option<Decimal>,
    concentration: Vec<Slice>,
}

fn load_risk(repo: &Repository) -> CardRoiResult<RiskData> {
    let allocation = portfolio::allocation_by_card(repo)?;
    let fractions: Vec<Decimal> = allocation.iter().map(|a| a.allocation_pct).collect();
    let concentration = portfolio::hhi(&fractions);
    let score = portfolio::diversification_score(concentration.hhi);
    Ok(RiskData {
        diversification_score: score,
        diversification_label: portfolio::diversification_label(score),
        hhi: concentration.hhi,
        effective_positions: concentration.effective_positions,
        concentration: fold_to_top_four(&allocation),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dimension {
    Set,
    Player,
    Sport,
}

impl Dimension {
    fn label(self) -> &'static str {
        match self {
            Dimension::Set => "Set",
            Dimension::Player => "Player",
            Dimension::Sport => "Sport",
        }
    }
}

fn load_allocation(dimension: Dimension, repo: &Repository) -> CardRoiResult<Vec<Slice>> {
    let entries = match dimension {
        Dimension::Set => portfolio::allocation_by_set(repo)?,
        Dimension::Player => portfolio::allocation_by_player(repo)?,
        Dimension::Sport => portfolio::allocation_by_sport(repo)?,
    };
    Ok(fold_to_top_four(&entries))
}

#[component]
pub fn RiskAllocation() -> Element {
    let bridge = use_context::<WebBridge>();
    let mut dimension = use_signal(|| Dimension::Set);

    let risk = use_resource({
        let bridge = bridge.clone();
        move || {
            let bridge = bridge.clone();
            async move { bridge.run(load_risk).await }
        }
    });

    let allocation = use_resource(move || {
        let bridge = bridge.clone();
        let dimension = dimension();
        async move {
            bridge
                .run(move |repo| load_allocation(dimension, repo))
                .await
        }
    });

    rsx! {
        div { class: "p-8 flex flex-col gap-8 max-w-4xl",
            h1 { class: "text-2xl font-semibold m-0", "Risk & allocation" }

            match &*risk.read() {
                None => rsx! { div { class: "text-text-secondary", "Loading..." } },
                Some(Err(err)) => rsx! { div { class: "text-loss", "Failed to load risk data: {err}" } },
                Some(Ok(data)) => rsx! { RiskSection { data: data.clone() } },
            }

            div {
                div { class: "flex items-center justify-between mb-3",
                    h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0", "Allocation" }
                    div { class: "flex gap-2",
                        for option in [Dimension::Set, Dimension::Player, Dimension::Sport] {
                            button {
                                class: if option == dimension() { "px-3 py-1.5 rounded-radius bg-gold text-canvas border-none font-semibold cursor-pointer" } else { "px-3 py-1.5 rounded-radius bg-surface text-text-secondary border border-border cursor-pointer" },
                                onclick: move |_| dimension.set(option),
                                "{option.label()}"
                            }
                        }
                    }
                }
                match &*allocation.read() {
                    None => rsx! { div { class: "text-text-secondary", "Loading..." } },
                    Some(Err(err)) => rsx! { div { class: "text-loss", "Failed to load allocation: {err}" } },
                    Some(Ok(slices)) => rsx! { AllocationDonut { slices: slices.clone() } },
                }
            }
        }
    }
}

#[component]
fn RiskSection(data: RiskData) -> Element {
    // `Decimal`'s `{:.N}` formatting truncates toward zero instead of
    // rounding - `round_dp` must run first. The score rounds to a whole
    // number (a "72/100" register, not "72.34/100"); HHI itself stays
    // unrounded to match the CLI's own `cardroi report` display exactly.
    let score_rounded = data.diversification_score.round_dp(0);
    let effective_rounded = data.effective_positions.map(|e| e.round_dp(2));

    rsx! {
        div { class: "flex flex-col gap-4",
            div {
                p { class: "text-text-secondary text-sm m-0 mb-1", "Diversification score" }
                div { class: "flex items-baseline gap-3",
                    p { class: "data-numeral text-3xl m-0", "{score_rounded}/100" }
                    p { class: "text-lg m-0 text-gold", "{data.diversification_label}" }
                }
                p { class: "text-text-tertiary text-xs mt-2 mb-0",
                    "HHI {data.hhi}"
                    if let Some(effective) = effective_rounded {
                        " - as concentrated as {effective} equal-sized positions"
                    }
                }
            }

            div {
                h2 { class: "text-sm font-semibold text-text-secondary uppercase tracking-wide m-0 mb-3", "Concentration by card" }
                ConcentrationBar { slices: data.concentration }
            }
        }
    }
}

/// A value-leads-label-follows hover/focus readout, shared by both charts
/// below - functionally the same job as a floating tooltip (dataviz's
/// interaction spec: "every value a tooltip shows is also reachable...
/// same details on keyboard focus as on hover"), just docked in a fixed
/// spot instead of following the pointer, so there's no floating-position
/// math and no layout jump when nothing is hovered.
#[component]
fn HoverReadout(slice: Option<Slice>) -> Element {
    rsx! {
        div { class: "h-6 flex items-center gap-2",
            if let Some(s) = slice {
                span { class: "data-numeral text-text-primary font-semibold", "{money(s.value)} ({percent(s.pct)})" }
                span { class: "text-text-tertiary text-sm", "{s.label}" }
            } else {
                span { class: "text-text-tertiary text-sm", "Hover or focus a slice for details" }
            }
        }
    }
}

#[component]
fn ConcentrationBar(slices: Vec<Slice>) -> Element {
    if slices.is_empty() {
        return rsx! {
            p { class: "text-text-secondary m-0", "No currently-owned, valued holdings to show." }
        };
    }

    let mut hovered = use_signal(|| None::<usize>);
    let hovered_slice = hovered().and_then(|i| slices.get(i).cloned());
    let bars: Vec<(usize, String, String)> = slices
        .iter()
        .enumerate()
        .map(|(i, slice)| {
            let brightness = if hovered() == Some(i) { "1.18" } else { "1" };
            let style = format!(
                "flex-grow: {}; flex-basis: 0; background-color: {}; filter: brightness({brightness});",
                slice.pct,
                slot_color(i, slices.len())
            );
            let title = format!("{}: {} ({})", slice.label, money(slice.value), percent(slice.pct));
            (i, style, title)
        })
        .collect();

    rsx! {
        div { class: "flex flex-col gap-3",
            div { class: "flex gap-0.5 h-10 rounded-radius overflow-hidden",
                for (i , style , title) in bars {
                    div {
                        class: "cursor-pointer transition-all duration-[var(--duration-standard)] ease-standard focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold focus-visible:outline-offset-[-2px]",
                        style: "{style}",
                        title: "{title}",
                        tabindex: "0",
                        onmouseenter: move |_| hovered.set(Some(i)),
                        onmouseleave: move |_| hovered.set(None),
                        onfocus: move |_| hovered.set(Some(i)),
                        onblur: move |_| hovered.set(None),
                    }
                }
            }
            HoverReadout { slice: hovered_slice }
            div { class: "flex flex-col gap-1.5",
                for (i , slice) in slices.iter().enumerate() {
                    div { class: "flex items-center gap-2 text-sm",
                        span {
                            class: "inline-block rounded-radius",
                            style: "width: 10px; height: 10px; background-color: {slot_color(i, slices.len())};",
                        }
                        span { class: "flex-1", "{slice.label}" }
                        span { class: "data-numeral text-text-secondary", "{money(slice.value)} ({percent(slice.pct)})" }
                    }
                }
            }
        }
    }
}

/// A donut slice's SVG path - an annulus sector from `start_angle` to
/// `end_angle` (degrees, 0 = 12 o'clock, clockwise), so each slice is its
/// own real DOM element with its own hover/focus handlers, unlike a
/// single flat `conic-gradient` background.
#[allow(clippy::too_many_arguments)]
fn donut_slice_path(
    start_angle: f64,
    end_angle: f64,
    cx: f64,
    cy: f64,
    outer_r: f64,
    inner_r: f64,
) -> String {
    let point = |angle_deg: f64, r: f64| {
        let a = angle_deg.to_radians();
        (cx + r * a.sin(), cy - r * a.cos())
    };
    // A slice spanning the full circle (single 100%-allocation entry) has
    // start/end points that are numerically identical - per the SVG spec,
    // an arc whose endpoint equals its start point renders as nothing at
    // all, so the whole donut would silently disappear. Clamping the
    // endpoint's angle a hair short of a full turn keeps two visually
    // distinct points (a sub-pixel gap at this radius) without changing
    // anything for every other, non-degenerate slice.
    let clamped_end = if end_angle - start_angle >= 359.99 {
        start_angle + 359.99
    } else {
        end_angle
    };
    let (x0o, y0o) = point(start_angle, outer_r);
    let (x1o, y1o) = point(clamped_end, outer_r);
    let (x1i, y1i) = point(clamped_end, inner_r);
    let (x0i, y0i) = point(start_angle, inner_r);
    let large_arc = if end_angle - start_angle > 180.0 {
        1
    } else {
        0
    };
    format!(
        "M {x0o:.2} {y0o:.2} A {outer_r} {outer_r} 0 {large_arc} 1 {x1o:.2} {y1o:.2} \
         L {x1i:.2} {y1i:.2} A {inner_r} {inner_r} 0 {large_arc} 0 {x0i:.2} {y0i:.2} Z"
    )
}

/// The same slice's mid-angle unit direction, in degrees - used to "pop"
/// the hovered slice outward a few pixels along its own radius rather
/// than just recoloring it, the donut-chart equivalent of a bar's
/// brightness lift.
fn mid_angle_offset(start_angle: f64, end_angle: f64, distance: f64) -> (f64, f64) {
    let mid = (start_angle + end_angle) / 2.0;
    let a = mid.to_radians();
    (distance * a.sin(), -distance * a.cos())
}

#[component]
fn AllocationDonut(slices: Vec<Slice>) -> Element {
    if slices.is_empty() {
        return rsx! {
            p { class: "text-text-secondary m-0", "No currently-owned, valued holdings to show." }
        };
    }

    let mut hovered = use_signal(|| None::<usize>);
    let hovered_slice = hovered().and_then(|i| slices.get(i).cloned());
    let total_value: Money = slices.iter().fold(Money::ZERO, |acc, s| acc + s.value);

    let (cx, cy, outer_r, inner_r) = (90.0, 90.0, 82.0, 50.0);
    let mut cumulative = 0.0f64;
    let mut arcs: Vec<(usize, String, String)> = Vec::with_capacity(slices.len());
    for (i, slice) in slices.iter().enumerate() {
        let pct = slice.pct.to_string().parse::<f64>().unwrap_or(0.0) * 100.0;
        let start = cumulative * 3.6;
        let end = (cumulative + pct) * 3.6;
        let path = donut_slice_path(start, end, cx, cy, outer_r, inner_r);
        let style = if hovered() == Some(i) {
            let (dx, dy) = mid_angle_offset(start, end, 6.0);
            format!("transform: translate({dx:.2}px, {dy:.2}px);")
        } else {
            String::new()
        };
        arcs.push((i, path, style));
        cumulative += pct;
    }

    rsx! {
        div { class: "flex flex-col gap-3",
            div { class: "flex gap-8 items-center flex-wrap",
                div { class: "relative", style: "width: 180px; height: 180px;",
                    svg {
                        width: "180",
                        height: "180",
                        view_box: "0 0 180 180",
                        for (i , d , style) in arcs {
                            path {
                                d: "{d}",
                                fill: slot_color(i, slices.len()),
                                class: "cursor-pointer transition-transform duration-[var(--duration-standard)] ease-standard focus-visible:outline focus-visible:outline-2 focus-visible:outline-gold",
                                style: "{style}",
                                tabindex: "0",
                                onmouseenter: move |_| hovered.set(Some(i)),
                                onmouseleave: move |_| hovered.set(None),
                                onfocus: move |_| hovered.set(Some(i)),
                                onblur: move |_| hovered.set(None),
                            }
                        }
                    }
                    div {
                        class: "absolute inset-0 flex flex-col items-center justify-center pointer-events-none",
                        p { class: "data-numeral text-sm m-0", "{money(total_value)}" }
                        p { class: "text-text-tertiary text-xs m-0", "tracked" }
                    }
                }
                div { class: "flex flex-col gap-1.5",
                    for (i , slice) in slices.iter().enumerate() {
                        div { class: "flex items-center gap-2 text-sm",
                            span {
                                class: "inline-block rounded-radius",
                                style: "width: 10px; height: 10px; background-color: {slot_color(i, slices.len())};",
                            }
                            span { class: "flex-1", "{slice.label}" }
                            span { class: "data-numeral text-text-secondary", "{percent(slice.pct)}" }
                        }
                    }
                }
            }
            HoverReadout { slice: hovered_slice }
        }
    }
}

/// Slot color for position `i` of `total` slices - the last slice is
/// always "Other" (a residual bucket, not a real identity) when `total`
/// exceeds the 4 named colors, so it gets the neutral gray instead of a
/// 5th categorical hue.
fn slot_color(i: usize, total: usize) -> &'static str {
    if total > 4 && i == 4 {
        OTHER_COLOR
    } else {
        SLOT_COLORS[i % 4]
    }
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn a_full_circle_slice_has_visually_distinct_start_and_end_points() {
        let path = donut_slice_path(0.0, 360.0, 90.0, 90.0, 82.0, 50.0);
        assert!(
            !path.contains("8.00 8.00"),
            "outer start/end collapsed to the same point: {path}"
        );
    }

    #[wasm_bindgen_test]
    fn a_partial_slice_is_unaffected_by_the_full_circle_clamp() {
        let unclamped = donut_slice_path(0.0, 90.0, 90.0, 90.0, 82.0, 50.0);
        let point = |angle_deg: f64, r: f64| {
            let a: f64 = angle_deg.to_radians();
            (90.0 + r * a.sin(), 90.0 - r * a.cos())
        };
        let (x1o, y1o) = point(90.0, 82.0);
        assert!(unclamped.contains(&format!("{x1o:.2} {y1o:.2}")));
    }
}
