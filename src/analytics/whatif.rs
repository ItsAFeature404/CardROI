//! What-if scenario modeling: simulates a hypothetical disposition of a
//! currently-owned holding without writing anything to the database.
//!
//! Reuses `analytics::roi::cost_basis_and_proceeds` and
//! `analytics::irr::{transactions_to_cash_flows, xirr}` directly on the
//! holding's real transactions plus one hypothetical disposition appended
//! in memory — there is no parallel P&L or IRR calculation path here.
//!
//! Per the SEC Investment Adviser Marketing Rule and GIPS, hypothetical
//! performance must (1) disclose the actual assumptions used to compute
//! it, not just carry a
//! label, and (2) never be structurally interchangeable with real
//! performance output. `WhatIfResult` therefore always carries every
//! assumed input (even ones the caller didn't explicitly set, like zero
//! fees) and uses field names distinct from `HoldingPnl`/`RollupPnl` on
//! purpose, so its JSON shape can't be mistaken for a real one.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::analytics::irr::{self, transactions_to_cash_flows};
use crate::analytics::roi::cost_basis_and_proceeds;
use crate::db::repository::Repository;
use crate::error::{CardRoiError, Result as CardRoiResult};
use crate::models::{HoldingStatus, Money};

/// A hypothetical sale to evaluate — mirrors the cost fields of a real
/// disposition transaction so the same `total()` math applies.
#[derive(Debug, Clone)]
pub struct HypotheticalSale {
    pub price: Money,
    pub fees: Money,
    pub shipping: Money,
    pub tax: Money,
    pub other_cost: Money,
    pub date: NaiveDate,
    /// How the price was sourced — printed in the output as one of the
    /// disclosed assumptions (see the module doc comment above).
    pub price_source: PriceSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PriceSource {
    Given,
    LatestAppraisal { appraised_date: NaiveDate },
}

#[derive(Debug, Clone, Serialize)]
pub struct WhatIfResult {
    pub holding_id: i64,
    /// Always `true` — present so a consumer that only checks for the
    /// field's existence, not its value, still can't mistake this for a
    /// real `HoldingPnl`/`RollupPnl`.
    pub hypothetical: bool,
    pub assumed_sale_date: NaiveDate,
    pub assumed_price: Money,
    pub assumed_price_source: String,
    pub assumed_fees: Money,
    pub assumed_shipping: Money,
    pub assumed_tax: Money,
    pub assumed_other_cost: Money,
    pub cost_basis: Money,
    pub hypothetical_net_proceeds: Money,
    pub hypothetical_realized_pnl: Money,
    pub hypothetical_roi_pct: Option<Decimal>,
    /// `None` when the resulting cash-flow pattern has no defined IRR
    /// (e.g. the hypothetical sale falls on the same date as the sole
    /// acquisition) — never fabricated as 0%.
    pub hypothetical_irr_pct: Option<Decimal>,
}

/// Simulates selling `holding_id` per `sale`, without persisting anything.
/// Only valid for a currently-owned holding — a sold holding already has
/// a real answer via `analytics::roi::holding_pnl`.
pub fn holding_whatif(
    repo: &Repository,
    holding_id: i64,
    sale: HypotheticalSale,
) -> CardRoiResult<WhatIfResult> {
    let holding = repo.get_holding(holding_id)?;
    if holding.status != HoldingStatus::Owned {
        return Err(CardRoiError::validation(format!(
            "holding {holding_id} is not currently owned (status: {}); \
             what-if only applies to owned holdings, use `roi` for its real, realized numbers",
            holding.status.as_str()
        )));
    }

    let transactions = repo.list_transactions_for_holding(holding_id)?;
    let (cost_basis, _real_proceeds) = cost_basis_and_proceeds(&transactions);

    let hypothetical_costs = sale.fees + sale.shipping + sale.tax + sale.other_cost;
    let hypothetical_net_proceeds = sale.price - hypothetical_costs;
    let hypothetical_realized_pnl = hypothetical_net_proceeds - cost_basis;
    let hypothetical_roi_pct = hypothetical_realized_pnl.ratio(cost_basis);

    let mut flows = transactions_to_cash_flows(&transactions);
    flows.push((sale.date, hypothetical_net_proceeds));
    flows.sort_by_key(|(date, _)| *date);
    let hypothetical_irr_pct = irr::xirr(&flows).ok();

    Ok(WhatIfResult {
        holding_id,
        hypothetical: true,
        assumed_sale_date: sale.date,
        assumed_price: sale.price,
        assumed_price_source: match sale.price_source {
            PriceSource::Given => "user-supplied hypothetical price".to_string(),
            PriceSource::LatestAppraisal { appraised_date } => {
                format!("latest comp ({appraised_date}), not a live market value")
            }
        },
        assumed_fees: sale.fees,
        assumed_shipping: sale.shipping,
        assumed_tax: sale.tax,
        assumed_other_cost: sale.other_cost,
        cost_basis,
        hypothetical_net_proceeds,
        hypothetical_realized_pnl,
        hypothetical_roi_pct,
        hypothetical_irr_pct,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{NewCard, NewHolding, NewSet, NewTransaction};

    fn money(s: &str) -> Money {
        Money::from_str(s).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
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

    fn given_sale(price: &str, date: NaiveDate) -> HypotheticalSale {
        HypotheticalSale {
            price: money(price),
            fees: Money::ZERO,
            shipping: Money::ZERO,
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            date,
            price_source: PriceSource::Given,
        }
    }

    #[test]
    fn computes_hypothetical_realized_pnl_and_roi_against_real_cost_basis() {
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
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();

        let result =
            holding_whatif(&repo, holding.id, given_sale("150.00", date(2026, 4, 1))).unwrap();

        // cost basis 105.00; hypothetical net proceeds 150.00 (no fees
        // assumed); hypothetical realized P&L 45.00.
        assert_eq!(result.cost_basis, money("105.00"));
        assert_eq!(result.hypothetical_net_proceeds, money("150.00"));
        assert_eq!(result.hypothetical_realized_pnl, money("45.00"));
        assert_eq!(
            result.hypothetical_roi_pct,
            Some(Decimal::from(45) / Decimal::from(105))
        );
        assert!(result.hypothetical);
    }

    #[test]
    fn hypothetical_costs_reduce_net_proceeds() {
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
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();

        let sale = HypotheticalSale {
            price: money("150.00"),
            fees: money("10.00"),
            shipping: money("5.00"),
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            date: date(2026, 4, 1),
            price_source: PriceSource::Given,
        };
        let result = holding_whatif(&repo, holding.id, sale).unwrap();

        // net proceeds = 150 - 10 - 5 = 135; P&L = 135 - 100 = 35.
        assert_eq!(result.hypothetical_net_proceeds, money("135.00"));
        assert_eq!(result.hypothetical_realized_pnl, money("35.00"));
    }

    #[test]
    fn computes_a_hypothetical_irr_matching_the_exact_ten_percent_reference() {
        let repo = repo();
        let card_id = seed_card(&repo);
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    transaction_date: date(2023, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();

        let result =
            holding_whatif(&repo, holding.id, given_sale("1100.00", date(2024, 1, 1))).unwrap();

        let irr_pct = result.hypothetical_irr_pct.unwrap();
        let diff = (irr_pct - Decimal::try_from(0.10000000000002678).unwrap()).abs();
        assert!(diff < Decimal::new(1, 6), "expected ~10%, got {irr_pct}");
    }

    #[test]
    fn same_day_sale_has_no_defined_irr_but_still_reports_pnl() {
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
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();

        let result =
            holding_whatif(&repo, holding.id, given_sale("150.00", date(2026, 1, 1))).unwrap();

        assert_eq!(result.hypothetical_realized_pnl, money("50.00"));
        assert_eq!(
            result.hypothetical_irr_pct, None,
            "same-day cash flows have no defined annualized rate - must not fabricate one"
        );
    }

    #[test]
    fn uses_latest_appraisal_as_the_price_when_requested() {
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
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("175.00"),
            appraised_date: date(2026, 3, 1),
            ..Default::default()
        })
        .unwrap();

        let sale = HypotheticalSale {
            price: money("175.00"),
            fees: Money::ZERO,
            shipping: Money::ZERO,
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            date: date(2026, 6, 1), // "today", independent of appraisal date
            price_source: PriceSource::LatestAppraisal {
                appraised_date: date(2026, 3, 1),
            },
        };
        let result = holding_whatif(&repo, holding.id, sale).unwrap();

        assert_eq!(result.assumed_sale_date, date(2026, 6, 1));
        assert!(result.assumed_price_source.contains("2026-03-01"));
        assert!(
            result
                .assumed_price_source
                .contains("not a live market value")
        );
    }

    #[test]
    fn rejects_a_holding_that_is_already_sold() {
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
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("150.00"),
            ..Default::default()
        })
        .unwrap();

        let err =
            holding_whatif(&repo, holding.id, given_sale("999.00", date(2026, 1, 1))).unwrap_err();
        assert!(err.to_string().contains("not currently owned"));
        assert!(err.to_string().contains("use `roi`"));
    }

    #[test]
    fn does_not_write_anything_to_the_database() {
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

        let before = repo.list_transactions_for_holding(holding.id).unwrap();
        holding_whatif(&repo, holding.id, given_sale("999.00", date(2026, 6, 1))).unwrap();
        let after = repo.list_transactions_for_holding(holding.id).unwrap();

        assert_eq!(
            before.len(),
            after.len(),
            "no transaction should be written"
        );
        let unchanged = repo.get_holding(holding.id).unwrap();
        assert_eq!(
            unchanged.status,
            HoldingStatus::Owned,
            "status must not change"
        );
    }
}
