//! Realized and unrealized P&L. CardROI has no market-price feed by
//! design, so an unrealized number only ever exists when the user has
//! recorded a manual appraisal for that holding — an owned holding with no
//! appraisal reports cost basis only. Every unrealized figure carries the
//! appraisal date it was derived from so callers can (and must) label it
//! as a user-supplied value as of that date, never as a live market value.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::repository::Repository;
use crate::error::Result;
use crate::models::{HoldingStatus, Money, TransactionType};

/// P&L for a single physical holding.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HoldingPnl {
    pub holding_id: i64,
    pub card_id: i64,
    pub status: HoldingStatus,
    /// Sum of acquisition + adjustment transaction totals.
    pub cost_basis: Money,
    /// Sum of disposition transaction totals (zero if never sold).
    pub proceeds: Money,
    /// `proceeds - cost_basis`, only when `status == Sold` — realized P&L
    /// is meaningless for a holding that hasn't been disposed of.
    pub realized_pnl: Option<Money>,
    /// `realized_pnl / cost_basis`, only when realized and cost basis is
    /// nonzero.
    pub roi_pct: Option<Decimal>,
    /// `latest_appraisal.value - cost_basis`, only when the holding is
    /// still owned AND has at least one appraisal on record. `None`
    /// otherwise — never fabricated from a status quo assumption.
    pub unrealized_pnl: Option<Money>,
    /// The date of the appraisal `unrealized_pnl` was derived from. Always
    /// `Some` exactly when `unrealized_pnl` is `Some` — every unrealized
    /// number must be attributable to a specific user-supplied appraisal.
    pub unrealized_pnl_as_of: Option<NaiveDate>,
    /// `unrealized_pnl / cost_basis`, only when unrealized and cost basis
    /// is nonzero.
    pub unrealized_roi_pct: Option<Decimal>,
    pub holding_period_days: Option<i64>,
}

/// Splits a holding's transactions into cost basis (acquisition +
/// adjustment totals) and proceeds (disposition totals). `pub(crate)` so
/// `analytics::whatif` can compute a hypothetical disposition's P&L
/// against the same real cost basis without a parallel calculation path.
pub(crate) fn cost_basis_and_proceeds(
    transactions: &[crate::models::Transaction],
) -> (Money, Money) {
    let mut cost_basis = Money::ZERO;
    let mut proceeds = Money::ZERO;
    for txn in transactions {
        match txn.transaction_type {
            TransactionType::Acquisition | TransactionType::Adjustment => {
                cost_basis += txn.total;
            }
            TransactionType::Disposition => proceeds += txn.total,
        }
    }
    (cost_basis, proceeds)
}

pub fn holding_pnl(repo: &Repository, holding_id: i64) -> Result<HoldingPnl> {
    let holding = repo.get_holding(holding_id)?;
    let transactions = repo.list_transactions_for_holding(holding_id)?;

    let (cost_basis, proceeds) = cost_basis_and_proceeds(&transactions);

    // Sold, Lost, and Damaged are all terminal statuses backed by a real
    // disposition transaction (see Repository::record_sale/record_loss), so
    // all three realize a P&L. A lost/damaged holding's "proceeds" are
    // whatever residual/salvage value and insurance recovery were recorded
    // (zero for a total loss).
    let (realized_pnl, roi_pct) = if holding.status.is_closed() {
        let pnl = proceeds - cost_basis;
        (Some(pnl), pnl.ratio(cost_basis))
    } else {
        (None, None)
    };

    let (unrealized_pnl, unrealized_pnl_as_of, unrealized_roi_pct) =
        if holding.status == HoldingStatus::Owned {
            match repo.latest_appraisal_for_holding(holding_id)? {
                Some(appraisal) => {
                    let pnl = appraisal.appraised_value - cost_basis;
                    (
                        Some(pnl),
                        Some(appraisal.appraised_date),
                        pnl.ratio(cost_basis),
                    )
                }
                None => (None, None, None),
            }
        } else {
            (None, None, None)
        };

    let holding_period_days = match (holding.acquired_date, holding.disposed_date) {
        (Some(acquired), Some(disposed)) => Some((disposed - acquired).num_days()),
        _ => None,
    };

    Ok(HoldingPnl {
        holding_id: holding.id,
        card_id: holding.card_id,
        status: holding.status,
        cost_basis,
        proceeds,
        realized_pnl,
        roi_pct,
        unrealized_pnl,
        unrealized_pnl_as_of,
        unrealized_roi_pct,
        holding_period_days,
    })
}

/// Aggregate P&L across a set of holdings (a card, a set, or the whole
/// portfolio).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RollupPnl {
    pub holding_count: usize,
    /// Holdings no longer owned — sold, lost, or damaged — each backed by a
    /// real disposition transaction. Was named `sold_count` before Lost/
    /// Damaged holdings recorded a real transaction; renamed since it now
    /// covers all three terminal statuses.
    pub closed_count: usize,
    /// Cost basis across every holding regardless of status.
    pub cost_basis: Money,
    /// Cost basis of currently-owned (not yet sold/lost/damaged) holdings —
    /// capital still deployed.
    pub open_cost_basis: Money,
    /// Sum of proceeds from closed holdings (sale price, or residual/
    /// insurance-recovery value for a lost/damaged holding).
    pub proceeds: Money,
    /// Sum of realized P&L from closed holdings.
    pub realized_pnl: Money,
    /// Fraction of closed holdings with strictly positive realized P&L.
    /// `None` if nothing has closed yet.
    pub win_rate: Option<Decimal>,
    /// Sum of `unrealized_pnl` across owned holdings that have at least one
    /// appraisal. Owned holdings with no appraisal contribute zero, not an
    /// estimate — see `appraised_open_count` to know how much of
    /// `open_cost_basis` this figure actually covers.
    pub unrealized_pnl: Money,
    /// How many owned holdings contributed to `unrealized_pnl` (i.e. have
    /// at least one appraisal on record), out of the total open holdings
    /// counted in `open_cost_basis`. `unrealized_pnl` is a partial figure,
    /// not a portfolio-wide unrealized total, whenever this is less than
    /// the number of open holdings.
    pub appraised_open_count: usize,
}

pub fn card_pnl(repo: &Repository, card_id: i64) -> Result<RollupPnl> {
    let holdings = repo.list_holdings(Some(card_id), None)?;
    rollup(repo, &holdings)
}

pub fn set_pnl(repo: &Repository, set_id: i64) -> Result<RollupPnl> {
    let cards = repo.list_cards(Some(set_id))?;
    let mut holdings = Vec::new();
    for card in cards {
        holdings.extend(repo.list_holdings(Some(card.id), None)?);
    }
    rollup(repo, &holdings)
}

pub fn portfolio_pnl(repo: &Repository) -> Result<RollupPnl> {
    let holdings = repo.list_holdings(None, None)?;
    rollup(repo, &holdings)
}

/// Aggregates P&L across an arbitrary list of holdings. One
/// `list_transactions_for_holding` query per holding — fine at the scale
/// tested here; if portfolio-level `roi`/`report` prove slow at 10k-100k+
/// holdings, replace with a single aggregate SQL query joining
/// holdings+transactions rather than N+1 lookups.
///
/// `pub(crate)` (not private) so `analytics::portfolio`'s player/sport
/// attribution rollups reuse this exact aggregation instead of a parallel
/// P&L calculation path.
pub(crate) fn rollup(repo: &Repository, holdings: &[crate::models::Holding]) -> Result<RollupPnl> {
    let mut closed_count = 0usize;
    let mut winning_count = 0usize;
    let mut cost_basis = Money::ZERO;
    let mut open_cost_basis = Money::ZERO;
    let mut proceeds = Money::ZERO;
    let mut realized_pnl = Money::ZERO;
    let mut unrealized_pnl = Money::ZERO;
    let mut appraised_open_count = 0usize;

    for holding in holdings {
        let pnl = holding_pnl(repo, holding.id)?;
        cost_basis += pnl.cost_basis;
        if pnl.status == HoldingStatus::Owned {
            open_cost_basis += pnl.cost_basis;
            if let Some(unrealized) = pnl.unrealized_pnl {
                unrealized_pnl += unrealized;
                appraised_open_count += 1;
            }
        }
        if let Some(realized) = pnl.realized_pnl {
            closed_count += 1;
            proceeds += pnl.proceeds;
            realized_pnl += realized;
            if realized.cents() > 0 {
                winning_count += 1;
            }
        }
    }

    let win_rate = if closed_count > 0 {
        Some(Decimal::from(winning_count) / Decimal::from(closed_count))
    } else {
        None
    };

    Ok(RollupPnl {
        holding_count: holdings.len(),
        closed_count,
        cost_basis,
        open_cost_basis,
        proceeds,
        realized_pnl,
        win_rate,
        unrealized_pnl,
        appraised_open_count,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::NaiveDate;

    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{NewCard, NewHolding, NewSet, NewTransaction};

    fn money(s: &str) -> Money {
        Money::from_str(s).unwrap()
    }

    fn repo() -> Repository {
        Repository::new(open_in_memory().unwrap())
    }

    fn seed_card(repo: &Repository) -> i64 {
        let set = repo
            .create_set(&NewSet {
                name: "2023 Topps Chrome".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_card(&NewCard {
            set_id: set.id,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn owned_holding_reports_cost_basis_with_no_pnl_claim() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    acquired_date: Some(date(2026, 1, 1)),
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    fees: money("5.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.cost_basis, money("105.00"));
        assert_eq!(pnl.proceeds, Money::ZERO);
        assert_eq!(
            pnl.realized_pnl, None,
            "an unsold holding must not claim a realized gain/loss"
        );
        assert_eq!(pnl.roi_pct, None);
        assert_eq!(
            pnl.unrealized_pnl, None,
            "no regression: an owned holding with no appraisal must not claim unrealized P&L"
        );
        assert_eq!(pnl.unrealized_pnl_as_of, None);
        assert_eq!(pnl.unrealized_roi_pct, None);
        assert_eq!(
            pnl.holding_period_days, None,
            "no disposed_date means no holding period yet"
        );
    }

    #[test]
    fn sold_holding_computes_exact_realized_pnl_and_roi() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    acquired_date: Some(date(2026, 1, 1)),
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    fees: money("5.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("150.00"),
            fees: money("10.00"),
            transaction_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        // cost basis: 100 + 5 = 105.00
        // proceeds: 150 - 10 = 140.00
        // realized P&L: 140.00 - 105.00 = 35.00
        // roi%: 35 / 105 = 1/3 exactly
        assert_eq!(pnl.cost_basis, money("105.00"));
        assert_eq!(pnl.proceeds, money("140.00"));
        assert_eq!(pnl.realized_pnl, Some(money("35.00")));
        assert_eq!(pnl.roi_pct, Some(Decimal::from(35) / Decimal::from(105)));
        // 2026-01-01 to 2026-04-01 = 90 days
        assert_eq!(pnl.holding_period_days, Some(90));
    }

    #[test]
    fn adjustment_transaction_adds_to_cost_basis() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        // e.g. a later grading submission fee.
        repo.create_transaction(&NewTransaction {
            holding_id: holding.id,
            transaction_type: TransactionType::Adjustment,
            price: money("20.00"),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.cost_basis, money("120.00"));
    }

    #[test]
    fn zero_cost_basis_does_not_panic_and_reports_no_roi_pct() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: Money::ZERO, // e.g. gifted card
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("50.00"),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.realized_pnl, Some(money("50.00")));
        assert_eq!(
            pnl.roi_pct, None,
            "roi% is undefined (not infinite) when cost basis is zero"
        );
    }

    #[test]
    fn card_pnl_rolls_up_mixed_owned_and_sold_holdings() {
        let repo = repo();
        let card_id = seed_card(&repo);

        // Holding 1: sold at a profit.
        let (h1, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: h1.id,
            price: money("150.00"),
            ..Default::default()
        })
        .unwrap();

        // Holding 2: still owned.
        repo.record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("40.00"),
                ..Default::default()
            },
        )
        .unwrap();

        let rollup = card_pnl(&repo, card_id).unwrap();

        assert_eq!(rollup.holding_count, 2);
        assert_eq!(rollup.closed_count, 1);
        assert_eq!(rollup.cost_basis, money("140.00")); // 100 + 40
        assert_eq!(rollup.open_cost_basis, money("40.00"));
        assert_eq!(rollup.proceeds, money("150.00"));
        assert_eq!(rollup.realized_pnl, money("50.00"));
        assert_eq!(rollup.win_rate, Some(Decimal::from(1))); // 1/1 sold holdings won
    }

    #[test]
    fn win_rate_reflects_losing_sales() {
        let repo = repo();
        let card_id = seed_card(&repo);

        let (h1, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: h1.id,
            price: money("50.00"), // sold at a loss
            ..Default::default()
        })
        .unwrap();

        let rollup = card_pnl(&repo, card_id).unwrap();

        assert_eq!(rollup.realized_pnl, money("-50.00"));
        assert_eq!(rollup.win_rate, Some(Decimal::ZERO));
    }

    #[test]
    fn win_rate_is_none_when_nothing_has_sold() {
        let repo = repo();
        let card_id = seed_card(&repo);
        repo.record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("10.00"),
                ..Default::default()
            },
        )
        .unwrap();

        let rollup = card_pnl(&repo, card_id).unwrap();

        assert_eq!(rollup.win_rate, None);
    }

    #[test]
    fn portfolio_pnl_aggregates_across_sets_and_cards() {
        let repo = repo();
        let card_id_a = seed_card(&repo);
        let set_b = repo
            .create_set(&NewSet {
                name: "2024 Bowman".to_string(),
                sport: "Baseball".to_string(),
                ..Default::default()
            })
            .unwrap();
        let card_id_b = repo
            .create_card(&NewCard {
                set_id: set_b.id,
                card_number: "1".to_string(),
                player_name: "Someone Else".to_string(),
                ..Default::default()
            })
            .unwrap()
            .id;

        repo.record_acquisition(
            &NewHolding {
                card_id: card_id_a,
                ..Default::default()
            },
            NewTransaction {
                price: money("10.00"),
                ..Default::default()
            },
        )
        .unwrap();
        repo.record_acquisition(
            &NewHolding {
                card_id: card_id_b,
                ..Default::default()
            },
            NewTransaction {
                price: money("20.00"),
                ..Default::default()
            },
        )
        .unwrap();

        let rollup = portfolio_pnl(&repo).unwrap();

        assert_eq!(rollup.holding_count, 2);
        assert_eq!(rollup.cost_basis, money("30.00"));
    }

    #[test]
    fn owned_holding_with_appraisal_computes_unrealized_pnl_and_roi() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    fees: money("5.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("140.00"),
            appraised_date: date(2026, 6, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        // cost basis: 100 + 5 = 105.00; unrealized: 140.00 - 105.00 = 35.00
        assert_eq!(pnl.unrealized_pnl, Some(money("35.00")));
        assert_eq!(pnl.unrealized_pnl_as_of, Some(date(2026, 6, 1)));
        assert_eq!(
            pnl.unrealized_roi_pct,
            Some(Decimal::from(35) / Decimal::from(105))
        );
        assert_eq!(
            pnl.realized_pnl, None,
            "still owned - unrealized is not realized"
        );
    }

    #[test]
    fn owned_holding_with_appraisal_below_cost_basis_shows_unrealized_loss() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("60.00"),
            appraised_date: date(2026, 6, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.unrealized_pnl, Some(money("-40.00")));
    }

    #[test]
    fn owned_holding_uses_latest_appraisal_by_date_not_insertion_order() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("200.00"),
            appraised_date: date(2026, 6, 1),
            ..Default::default()
        })
        .unwrap();
        // Inserted after, but dated earlier - must not override the later one.
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("120.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.unrealized_pnl, Some(money("100.00")));
        assert_eq!(pnl.unrealized_pnl_as_of, Some(date(2026, 6, 1)));
    }

    #[test]
    fn sold_holding_never_claims_unrealized_pnl_even_with_a_stale_appraisal() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("140.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("150.00"),
            transaction_date: date(2026, 3, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(
            pnl.unrealized_pnl, None,
            "a sold holding reports realized P&L only, never unrealized, regardless of any old appraisal on file"
        );
        assert_eq!(pnl.realized_pnl, Some(money("50.00")));
    }

    #[test]
    fn zero_cost_basis_with_appraisal_reports_no_unrealized_roi_pct() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: Money::ZERO, // e.g. gifted card
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("50.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        assert_eq!(pnl.unrealized_pnl, Some(money("50.00")));
        assert_eq!(
            pnl.unrealized_roi_pct, None,
            "roi% is undefined (not infinite) when cost basis is zero"
        );
    }

    #[test]
    fn card_pnl_rolls_up_unrealized_pnl_only_from_appraised_open_holdings() {
        let repo = repo();
        let card_id = seed_card(&repo);

        // Holding 1: owned, appraised - contributes to unrealized_pnl.
        let (h1, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: h1.id,
            appraised_value: money("150.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();

        // Holding 2: owned, no appraisal - contributes cost basis but not
        // unrealized_pnl, and must not silently be treated as zero gain.
        repo.record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("40.00"),
                ..Default::default()
            },
        )
        .unwrap();

        let rollup = card_pnl(&repo, card_id).unwrap();

        assert_eq!(rollup.holding_count, 2);
        assert_eq!(rollup.open_cost_basis, money("140.00")); // 100 + 40
        assert_eq!(rollup.unrealized_pnl, money("50.00")); // only h1's 150 - 100
        assert_eq!(
            rollup.appraised_open_count, 1,
            "only 1 of the 2 open holdings has an appraisal on record"
        );
    }

    #[test]
    fn lost_holding_with_no_residual_value_realizes_a_full_loss() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();

        repo.record_loss(
            holding.id,
            HoldingStatus::Lost,
            date(2026, 2, 1),
            Money::ZERO,
            Money::ZERO,
            Some("stolen".to_string()),
            None,
        )
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        // Before the fix, a Lost holding's cost basis vanished from every
        // rollup with no realized loss recorded anywhere - this is the bug
        // this test guards against regressing.
        assert_eq!(pnl.status, HoldingStatus::Lost);
        assert_eq!(pnl.proceeds, Money::ZERO);
        assert_eq!(
            pnl.realized_pnl,
            Some(money("-100.00")),
            "a total loss must realize the full cost basis as a loss, not vanish"
        );
        assert_eq!(pnl.roi_pct, Some(Decimal::from(-1)));
    }

    #[test]
    fn damaged_holding_with_residual_value_realizes_a_partial_loss() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();

        repo.record_loss(
            holding.id,
            HoldingStatus::Damaged,
            date(2026, 2, 1),
            money("20.00"), // residual/salvage value retained
            money("30.00"), // insurance recovery
            Some("water damage".to_string()),
            None,
        )
        .unwrap();

        let pnl = holding_pnl(&repo, holding.id).unwrap();

        // proceeds: 20.00 (residual) + 30.00 (insurance) = 50.00
        // realized P&L: 50.00 - 100.00 = -50.00
        assert_eq!(pnl.proceeds, money("50.00"));
        assert_eq!(pnl.realized_pnl, Some(money("-50.00")));
        assert_eq!(pnl.roi_pct, Some(-Decimal::from(1) / Decimal::from(2)));
    }

    #[test]
    fn card_pnl_rollup_counts_lost_and_damaged_holdings_as_closed_not_open() {
        let repo = repo();
        let card_id = seed_card(&repo);

        // Holding 1: lost, total loss.
        let (h1, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_loss(
            h1.id,
            HoldingStatus::Lost,
            date(2026, 1, 1),
            Money::ZERO,
            Money::ZERO,
            None,
            None,
        )
        .unwrap();

        // Holding 2: still owned.
        repo.record_acquisition(
            &NewHolding {
                card_id,
                ..Default::default()
            },
            NewTransaction {
                price: money("40.00"),
                ..Default::default()
            },
        )
        .unwrap();

        let rollup = card_pnl(&repo, card_id).unwrap();

        assert_eq!(rollup.holding_count, 2);
        assert_eq!(
            rollup.closed_count, 1,
            "a lost holding is closed, same as a sold one"
        );
        assert_eq!(
            rollup.open_cost_basis,
            money("40.00"),
            "a lost holding's cost basis must not still count as capital deployed"
        );
        assert_eq!(rollup.realized_pnl, money("-100.00"));
        assert_eq!(
            rollup.win_rate,
            Some(Decimal::ZERO),
            "a total loss counts as a non-winning closed position"
        );
    }
}
