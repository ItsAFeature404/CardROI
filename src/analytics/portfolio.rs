//! Portfolio-level analytics: allocation, concentration risk (HHI), and
//! P&L attribution by player/sport. Attribution reuses `analytics::roi`'s
//! `rollup` — the same building block `card_pnl`/`set_pnl`/`portfolio_pnl`
//! already use — so there is exactly one P&L calculation path in the
//! whole crate, not a parallel one for this module.
//!
//! Allocation weights each currently-owned holding by its latest appraised
//! value where one exists, else its cost basis (never a fabricated
//! estimate) — matching the same "appraisal if present, else cost basis"
//! convention `analytics::roi::HoldingPnl` already uses. Sold/lost/damaged
//! holdings are excluded: allocation describes today's portfolio
//! composition, not historical capital deployment (`RollupPnl::cost_basis`
//! already covers the all-time view).

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::repository::Repository;
use crate::error::Result;
use crate::models::{HoldingStatus, Money};

use super::roi::{self, HoldingPnl, RollupPnl};

/// One bucket's share of the currently-owned portfolio.
#[derive(Debug, Clone, Serialize)]
pub struct AllocationEntry {
    pub label: String,
    /// Latest appraised value where available, else cost basis, summed
    /// across every owned holding in this bucket.
    pub value: Money,
    /// `value / portfolio_total`, as a fraction (e.g. `0.25`, not `25`) —
    /// zero when the portfolio total is zero.
    pub allocation_pct: Decimal,
}

/// Concentration risk across a set of allocation fractions: the standard
/// Herfindahl-Hirschman Index, the sum of squared fractions. Ranges from
/// near-zero (many small, even positions) to `1` (a single position holds
/// everything). `effective_positions = 1 / hhi` is the "as if N
/// equal-sized positions" reading of the same number — `None` only when
/// there's nothing allocated at all (`hhi == 0`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Concentration {
    pub hhi: Decimal,
    pub effective_positions: Option<Decimal>,
}

pub fn hhi(fractions: &[Decimal]) -> Concentration {
    let hhi: Decimal = fractions.iter().map(|f| f * f).sum();
    let effective_positions = if hhi.is_zero() {
        None
    } else {
        Some(Decimal::ONE / hhi)
    };
    Concentration {
        hhi,
        effective_positions,
    }
}

/// Converts a raw HHI (0-1, higher = more concentrated) into a 0-100
/// diversification score (higher = more diversified) for plain-language
/// display - the raw index has no intuitive meaning to a non-technical
/// collector on its own (a 0.05 HHI and a 0.5 HHI mean very different
/// things, but neither number says so by itself).
pub fn diversification_score(hhi: Decimal) -> Decimal {
    ((Decimal::ONE - hhi) * Decimal::from(100)).clamp(Decimal::ZERO, Decimal::from(100))
}

/// Plain-language reading of a `diversification_score`. Thresholds chosen
/// so a single dominant position (HHI >= 0.5, score <= 50) always reads as
/// "highly concentrated", and a portfolio spread across several roughly
/// even positions (HHI <= 0.2, score >= 80) reads as "well diversified".
pub fn diversification_label(score: Decimal) -> &'static str {
    if score >= Decimal::from(80) {
        "Well diversified"
    } else if score >= Decimal::from(50) {
        "Moderately concentrated"
    } else {
        "Highly concentrated"
    }
}

/// P&L attribution for one grouping bucket (a player or a sport), reusing
/// the exact same `RollupPnl` shape as the portfolio/card/set rollups.
#[derive(Debug, Clone, Serialize)]
pub struct AttributionEntry {
    pub label: String,
    pub pnl: RollupPnl,
}

/// The "current value" of an owned holding for allocation purposes: its
/// latest appraisal if one exists (`cost_basis + unrealized_pnl` recovers
/// the appraised value exactly, without a second repository round trip),
/// else cost basis.
fn holding_value(pnl: &HoldingPnl) -> Money {
    match pnl.unrealized_pnl {
        Some(unrealized) => pnl.cost_basis + unrealized,
        None => pnl.cost_basis,
    }
}

/// `(card_id, current_value)` for every currently-owned holding.
fn owned_holding_values(repo: &Repository) -> Result<Vec<(i64, Money)>> {
    let holdings = repo.list_holdings(None, Some(HoldingStatus::Owned))?;
    let mut values = Vec::with_capacity(holdings.len());
    for holding in &holdings {
        let pnl = roi::holding_pnl(repo, holding.id)?;
        values.push((holding.card_id, holding_value(&pnl)));
    }
    Ok(values)
}

fn finalize_allocation(buckets: HashMap<String, Money>) -> Vec<AllocationEntry> {
    let total = buckets
        .values()
        .copied()
        .fold(Money::ZERO, |acc, v| acc + v);
    let mut entries: Vec<AllocationEntry> = buckets
        .into_iter()
        .map(|(label, value)| AllocationEntry {
            label,
            value,
            allocation_pct: value.ratio(total).unwrap_or(Decimal::ZERO),
        })
        .collect();
    entries.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
    entries
}

/// Allocation by card, across currently-owned holdings only.
pub fn allocation_by_card(repo: &Repository) -> Result<Vec<AllocationEntry>> {
    let cards: HashMap<i64, String> = repo
        .list_cards(None)?
        .into_iter()
        .map(|c| (c.id, c.display_name()))
        .collect();

    let mut buckets: HashMap<String, Money> = HashMap::new();
    for (card_id, value) in owned_holding_values(repo)? {
        let label = cards
            .get(&card_id)
            .cloned()
            .unwrap_or_else(|| format!("card #{card_id}"));
        *buckets.entry(label).or_insert(Money::ZERO) += value;
    }
    Ok(finalize_allocation(buckets))
}

/// Allocation by set, across currently-owned holdings only.
pub fn allocation_by_set(repo: &Repository) -> Result<Vec<AllocationEntry>> {
    let card_to_set: HashMap<i64, i64> = repo
        .list_cards(None)?
        .into_iter()
        .map(|c| (c.id, c.set_id))
        .collect();
    let set_names: HashMap<i64, String> = repo
        .list_sets()?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();

    let mut buckets: HashMap<String, Money> = HashMap::new();
    for (card_id, value) in owned_holding_values(repo)? {
        let label = card_to_set
            .get(&card_id)
            .and_then(|set_id| set_names.get(set_id))
            .cloned()
            .unwrap_or_else(|| "Unknown set".to_string());
        *buckets.entry(label).or_insert(Money::ZERO) += value;
    }
    Ok(finalize_allocation(buckets))
}

/// Allocation by player, across currently-owned holdings only.
pub fn allocation_by_player(repo: &Repository) -> Result<Vec<AllocationEntry>> {
    let card_to_player: HashMap<i64, String> = repo
        .list_cards(None)?
        .into_iter()
        .map(|c| (c.id, c.player_name))
        .collect();

    let mut buckets: HashMap<String, Money> = HashMap::new();
    for (card_id, value) in owned_holding_values(repo)? {
        let label = card_to_player
            .get(&card_id)
            .cloned()
            .unwrap_or_else(|| "Unknown player".to_string());
        *buckets.entry(label).or_insert(Money::ZERO) += value;
    }
    Ok(finalize_allocation(buckets))
}

/// Allocation by sport (via each card's set), across currently-owned
/// holdings only.
pub fn allocation_by_sport(repo: &Repository) -> Result<Vec<AllocationEntry>> {
    let sport_of_set: HashMap<i64, String> = repo
        .list_sets()?
        .into_iter()
        .map(|s| (s.id, s.sport))
        .collect();
    let card_to_sport: HashMap<i64, String> = repo
        .list_cards(None)?
        .into_iter()
        .map(|c| {
            let sport = sport_of_set
                .get(&c.set_id)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string());
            (c.id, sport)
        })
        .collect();

    let mut buckets: HashMap<String, Money> = HashMap::new();
    for (card_id, value) in owned_holding_values(repo)? {
        let label = card_to_sport
            .get(&card_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());
        *buckets.entry(label).or_insert(Money::ZERO) += value;
    }
    Ok(finalize_allocation(buckets))
}

/// Concentration risk over the by-card allocation — the standard reading
/// of "how concentrated is this portfolio in any single position."
pub fn concentration_by_card(repo: &Repository) -> Result<Concentration> {
    let allocation = allocation_by_card(repo)?;
    let fractions: Vec<Decimal> = allocation.iter().map(|a| a.allocation_pct).collect();
    Ok(hhi(&fractions))
}

fn cards_grouped_by<K, F>(repo: &Repository, key_of: F) -> Result<HashMap<K, Vec<i64>>>
where
    K: Eq + std::hash::Hash,
    F: Fn(&crate::models::Card, &HashMap<i64, String>) -> K,
{
    let sport_of_set: HashMap<i64, String> = repo
        .list_sets()?
        .into_iter()
        .map(|s| (s.id, s.sport))
        .collect();
    let mut groups: HashMap<K, Vec<i64>> = HashMap::new();
    for card in repo.list_cards(None)? {
        let key = key_of(&card, &sport_of_set);
        groups.entry(key).or_default().push(card.id);
    }
    Ok(groups)
}

fn attribution_from_groups(
    repo: &Repository,
    groups: HashMap<String, Vec<i64>>,
) -> Result<Vec<AttributionEntry>> {
    let mut entries = Vec::with_capacity(groups.len());
    for (label, card_ids) in groups {
        let mut holdings = Vec::new();
        for card_id in card_ids {
            holdings.extend(repo.list_holdings(Some(card_id), None)?);
        }
        entries.push(AttributionEntry {
            label,
            pnl: roi::rollup(repo, &holdings)?,
        });
    }
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(entries)
}

/// P&L attribution grouped by player, across every holding (owned and
/// sold alike — attribution is an all-time view, matching `RollupPnl`).
pub fn attribution_by_player(repo: &Repository) -> Result<Vec<AttributionEntry>> {
    let groups = cards_grouped_by(repo, |card, _| card.player_name.clone())?;
    attribution_from_groups(repo, groups)
}

/// P&L attribution grouped by sport (via each card's set).
pub fn attribution_by_sport(repo: &Repository) -> Result<Vec<AttributionEntry>> {
    let groups = cards_grouped_by(repo, |card, sport_of_set| {
        sport_of_set
            .get(&card.set_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string())
    })?;
    attribution_from_groups(repo, groups)
}

/// P&L attribution grouped by set. Doesn't reuse `cards_grouped_by` (that
/// helper's auxiliary lookup is specifically a set-to-sport map for the
/// sport case) - grouping by set only needs `card.set_id` resolved to a
/// name, no cross-table lookup indirection beyond that.
pub fn attribution_by_set(repo: &Repository) -> Result<Vec<AttributionEntry>> {
    let set_name_of: HashMap<i64, String> = repo
        .list_sets()?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let mut groups: HashMap<String, Vec<i64>> = HashMap::new();
    for card in repo.list_cards(None)? {
        let label = set_name_of
            .get(&card.set_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());
        groups.entry(label).or_default().push(card.id);
    }
    attribution_from_groups(repo, groups)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{NewAppraisal, NewCard, NewHolding, NewSet, NewTransaction};

    fn money(s: &str) -> Money {
        Money::from_str(s).unwrap()
    }

    fn repo() -> Repository {
        Repository::new(open_in_memory().unwrap())
    }

    fn seed_card(repo: &Repository, sport: &str, set_name: &str, player: &str) -> i64 {
        let set = repo
            .create_set(&NewSet {
                name: set_name.to_string(),
                sport: sport.to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_card(&NewCard {
            set_id: set.id,
            card_number: "1".to_string(),
            player_name: player.to_string(),
            ..Default::default()
        })
        .unwrap()
        .id
    }

    fn buy(repo: &Repository, card_id: i64, price: &str) -> i64 {
        repo.record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money(price),
                ..Default::default()
            },
        )
        .unwrap()
        .0
        .id
    }

    // --- hhi: hand-computed concentrated vs. diversified fixtures ---

    #[test]
    fn single_holding_portfolio_has_maximum_concentration() {
        // All-one-position: HHI = 1.0^2 = 1.0 exactly, effective_positions = 1.
        let result = hhi(&[Decimal::ONE]);
        assert_eq!(result.hhi, Decimal::ONE);
        assert_eq!(result.effective_positions, Some(Decimal::ONE));
    }

    #[test]
    fn twenty_even_positions_has_low_concentration() {
        // 20 equal 5% positions: HHI = 20 * (0.05)^2 = 0.05, effective
        // positions = 1/0.05 = 20 - the index recovers the true count
        // exactly when positions are perfectly even.
        let fractions: Vec<Decimal> = (0..20).map(|_| Decimal::new(5, 2)).collect();
        let result = hhi(&fractions);
        assert_eq!(result.hhi, Decimal::new(5, 2));
        assert_eq!(result.effective_positions, Some(Decimal::from(20)));
    }

    #[test]
    fn concentrated_portfolio_has_much_higher_hhi_than_diversified() {
        let concentrated = hhi(&[Decimal::new(90, 2), Decimal::new(10, 2)]); // 90%/10%
        let diversified: Vec<Decimal> = (0..10).map(|_| Decimal::new(10, 2)).collect(); // 10x10%
        let diversified = hhi(&diversified);
        assert!(concentrated.hhi > diversified.hhi);
        assert_eq!(diversified.hhi, Decimal::new(1, 1)); // 0.10
    }

    #[test]
    fn empty_portfolio_has_no_effective_positions() {
        let result = hhi(&[]);
        assert_eq!(result.hhi, Decimal::ZERO);
        assert_eq!(result.effective_positions, None);
    }

    // --- diversification_score: hand-computed ---

    #[test]
    fn single_position_scores_zero() {
        // HHI = 1.0 (all-one-position) -> (1 - 1.0) * 100 = 0.
        assert_eq!(diversification_score(Decimal::ONE), Decimal::ZERO);
    }

    #[test]
    fn twenty_even_positions_scores_ninety_five() {
        // HHI = 0.05 (from twenty_even_positions_has_low_concentration above)
        // -> (1 - 0.05) * 100 = 95.
        assert_eq!(diversification_score(Decimal::new(5, 2)), Decimal::from(95));
    }

    #[test]
    fn score_clamps_to_the_zero_to_one_hundred_range() {
        // HHI can't legitimately exceed 1 or go negative, but the clamp is
        // the actual safety net under test, not the impossible input.
        assert_eq!(diversification_score(Decimal::new(11, 1)), Decimal::ZERO); // hhi = 1.1
        assert_eq!(
            diversification_score(-Decimal::ONE),
            Decimal::from(100) // hhi = -1
        );
    }

    #[test]
    fn diversification_label_matches_hand_picked_thresholds() {
        assert_eq!(diversification_label(Decimal::from(95)), "Well diversified");
        assert_eq!(diversification_label(Decimal::from(80)), "Well diversified");
        assert_eq!(
            diversification_label(Decimal::new(799, 1)), // 79.9
            "Moderately concentrated"
        );
        assert_eq!(
            diversification_label(Decimal::from(50)),
            "Moderately concentrated"
        );
        assert_eq!(
            diversification_label(Decimal::new(499, 1)), // 49.9
            "Highly concentrated"
        );
        assert_eq!(diversification_label(Decimal::ZERO), "Highly concentrated");
    }

    // --- allocation: repo-facing ---

    #[test]
    fn allocation_by_card_sums_to_one_within_rounding_tolerance() {
        let repo = repo();
        // Deliberately uses thirds, which don't terminate in decimal - the
        // property under test is that rounding error stays negligible,
        // not that the sum is bit-exact.
        for _ in 0..3 {
            let card = seed_card(&repo, "Basketball", "2023 Topps Chrome", "Player");
            buy(&repo, card, "100.00");
        }

        let allocation = allocation_by_card(&repo).unwrap();
        let total: Decimal = allocation.iter().map(|a| a.allocation_pct).sum();
        let diff = (total - Decimal::ONE).abs();
        assert!(
            diff < Decimal::new(1, 20),
            "allocation percentages should sum to ~100%, got {total} (diff {diff})"
        );
    }

    #[test]
    fn allocation_by_card_uses_appraised_value_over_cost_basis_when_present() {
        let repo = repo();
        let card_a = seed_card(&repo, "Basketball", "2023 Topps Chrome", "Player A");
        let holding_a = buy(&repo, card_a, "100.00");
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_a,
            appraised_value: money("300.00"),
            appraised_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            ..Default::default()
        })
        .unwrap();
        let card_b = seed_card(&repo, "Basketball", "2024 Bowman", "Player B");
        buy(&repo, card_b, "100.00");

        let allocation = allocation_by_card(&repo).unwrap();
        let a_entry = allocation
            .iter()
            .find(|e| e.label.contains("Player A"))
            .unwrap();
        let b_entry = allocation
            .iter()
            .find(|e| e.label.contains("Player B"))
            .unwrap();

        // A is appraised at 300 (not its 100 cost basis), B stays at its
        // 100 cost basis - total tracked value is 400, so A is 75%.
        assert_eq!(a_entry.value, money("300.00"));
        assert_eq!(b_entry.value, money("100.00"));
        assert_eq!(a_entry.allocation_pct, Decimal::new(75, 2));
    }

    #[test]
    fn allocation_excludes_sold_holdings() {
        let repo = repo();
        let card = seed_card(&repo, "Basketball", "2023 Topps Chrome", "Player");
        let holding_id = buy(&repo, card, "100.00");
        repo.record_sale(NewTransaction {
            holding_id,
            price: money("150.00"),
            ..Default::default()
        })
        .unwrap();

        let allocation = allocation_by_card(&repo).unwrap();
        assert!(
            allocation.is_empty(),
            "a fully-sold portfolio has no current allocation"
        );
    }

    #[test]
    fn allocation_by_set_groups_multiple_cards_in_the_same_set() {
        let repo = repo();
        let set = repo
            .create_set(&NewSet {
                name: "2023 Topps Chrome".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        let card_a = repo
            .create_card(&NewCard {
                set_id: set.id,
                card_number: "1".to_string(),
                player_name: "Player A".to_string(),
                ..Default::default()
            })
            .unwrap()
            .id;
        let card_b = repo
            .create_card(&NewCard {
                set_id: set.id,
                card_number: "2".to_string(),
                player_name: "Player B".to_string(),
                ..Default::default()
            })
            .unwrap()
            .id;
        buy(&repo, card_a, "100.00");
        buy(&repo, card_b, "50.00");

        let allocation = allocation_by_set(&repo).unwrap();
        assert_eq!(allocation.len(), 1);
        assert_eq!(allocation[0].label, "2023 Topps Chrome");
        assert_eq!(allocation[0].value, money("150.00"));
    }

    #[test]
    fn allocation_by_player_groups_multiple_cards_of_the_same_player() {
        let repo = repo();
        let card_a = seed_card(&repo, "Basketball", "2023 Topps Chrome", "LeBron James");
        let card_b = seed_card(&repo, "Basketball", "2024 Bowman", "LeBron James");
        buy(&repo, card_a, "100.00");
        buy(&repo, card_b, "50.00");

        let allocation = allocation_by_player(&repo).unwrap();
        assert_eq!(allocation.len(), 1);
        assert_eq!(allocation[0].label, "LeBron James");
        assert_eq!(allocation[0].value, money("150.00"));
    }

    #[test]
    fn allocation_by_sport_groups_across_sets_of_the_same_sport() {
        let repo = repo();
        let card_a = seed_card(&repo, "Baseball", "2023 Topps Chrome", "Player A");
        let card_b = seed_card(&repo, "Baseball", "2024 Bowman", "Player B");
        buy(&repo, card_a, "100.00");
        buy(&repo, card_b, "50.00");

        let allocation = allocation_by_sport(&repo).unwrap();
        assert_eq!(allocation.len(), 1);
        assert_eq!(allocation[0].label, "Baseball");
        assert_eq!(allocation[0].value, money("150.00"));
    }

    // --- attribution: reuses roi::rollup, verified against hand-computed P&L ---

    #[test]
    fn attribution_by_player_matches_hand_computed_realized_pnl() {
        let repo = repo();
        let card = seed_card(&repo, "Basketball", "2023 Topps Chrome", "LeBron James");
        let holding_id = buy(&repo, card, "100.00");
        repo.record_sale(NewTransaction {
            holding_id,
            price: money("150.00"),
            ..Default::default()
        })
        .unwrap();

        let attribution = attribution_by_player(&repo).unwrap();
        assert_eq!(attribution.len(), 1);
        assert_eq!(attribution[0].label, "LeBron James");
        assert_eq!(attribution[0].pnl.realized_pnl, money("50.00"));
    }

    #[test]
    fn attribution_by_player_aggregates_across_multiple_cards_of_the_same_player() {
        let repo = repo();
        let card_a = seed_card(&repo, "Basketball", "2023 Topps Chrome", "LeBron James");
        let card_b = seed_card(&repo, "Basketball", "2024 Bowman", "LeBron James");
        buy(&repo, card_a, "100.00");
        buy(&repo, card_b, "50.00");

        let attribution = attribution_by_player(&repo).unwrap();
        assert_eq!(attribution.len(), 1);
        assert_eq!(attribution[0].pnl.cost_basis, money("150.00"));
        assert_eq!(attribution[0].pnl.holding_count, 2);
    }

    #[test]
    fn attribution_by_sport_matches_card_pnl_totals_for_a_single_sport_portfolio() {
        let repo = repo();
        let card = seed_card(&repo, "Baseball", "2024 Bowman", "Rookie Player");
        let holding_id = buy(&repo, card, "20.00");
        repo.record_sale(NewTransaction {
            holding_id,
            price: money("30.00"),
            ..Default::default()
        })
        .unwrap();

        let attribution = attribution_by_sport(&repo).unwrap();
        assert_eq!(attribution.len(), 1);
        assert_eq!(attribution[0].label, "Baseball");
        assert_eq!(attribution[0].pnl.realized_pnl, money("10.00"));
    }

    #[test]
    fn attribution_by_set_aggregates_across_multiple_players_in_the_same_set() {
        let repo = repo();
        let card_a = seed_card(&repo, "Basketball", "2023 Topps Chrome", "LeBron James");
        let card_b = seed_card(&repo, "Basketball", "2023 Topps Chrome", "Stephen Curry");
        buy(&repo, card_a, "100.00");
        buy(&repo, card_b, "50.00");

        let attribution = attribution_by_set(&repo).unwrap();
        assert_eq!(attribution.len(), 1);
        assert_eq!(attribution[0].label, "2023 Topps Chrome");
        assert_eq!(attribution[0].pnl.cost_basis, money("150.00"));
        assert_eq!(attribution[0].pnl.holding_count, 2);
    }
}
