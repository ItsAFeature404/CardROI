//! Time-Weighted Return (TWR) — chains sub-period returns between
//! consecutive appraisals, isolating investment *performance* from the
//! timing and size of the user's own contributions. That's the key
//! difference from `analytics::irr`: IRR is sensitive to when and how much
//! money the user put in or took out; TWR deliberately is not. The two
//! numbers answering different questions is a common source of confusion —
//! `cardroi twr --help` explains it, and the CLI shows both together.
//!
//! Unlike IRR, an un-annualized TWR has a closed form (a product of
//! sub-period ratios) — no root-finding. It stays in exact
//! `rust_decimal::Decimal` the whole way. Annualizing over a period
//! requires a fractional-exponent power with no exact decimal
//! representation for an irrational result; that one narrow operation
//! converts to `f64`, the same documented exception `analytics::irr` takes
//! for its root-finder.
//!
//! Sub-period convention (cross-checked against the `rust_finprim` crate's
//! `rate::twr` implementation and its own test fixtures — see this
//! module's tests below): each sub-period pairs an ending valuation with
//! the net external cash flow that occurred during that sub-period. A
//! positive cash flow is money
//! added to the tracked value (e.g. an additional cost-basis adjustment,
//! or a holding entering a shared timeline for the first time); it's
//! subtracted from the ending value before comparing to the starting
//! value, so it inflates the raw value change without counting as return.
//! A negative cash flow (e.g. a disposition) works the same way in
//! reverse — it's added back so a withdrawal doesn't read as a loss.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::db::repository::Repository;
use crate::error::{CardRoiError, Result as CardRoiResult};
use crate::models::{Appraisal, HoldingStatus, Money, Transaction, TransactionType};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TwrError {
    /// Fewer than 2 valuation points — no sub-period exists to chain.
    InsufficientPeriods,
    /// A sub-period's starting value was zero, making its return
    /// undefined (mirrors `rust_finprim`'s `DivideByZero`) — not
    /// fabricated as 0% or skipped silently.
    ZeroStartingValue { period_index: usize },
}

impl std::fmt::Display for TwrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TwrError::InsufficientPeriods => write!(
                f,
                "at least 2 valuation points are required to compute a time-weighted return"
            ),
            TwrError::ZeroStartingValue { period_index } => write!(
                f,
                "sub-period {period_index} started from a zero valuation; its return is undefined"
            ),
        }
    }
}

/// Chains sub-period returns into a whole-period time-weighted return.
///
/// `periods[i].0` is the valuation at the end of sub-period `i`;
/// `periods[i].1` is the net external cash flow during that sub-period.
/// `periods[0].1` is never read — the first entry only anchors the
/// starting value for sub-period 1. Requires `periods.len() >= 2`.
///
/// `annualize_over_years`, when given, converts the whole-period return to
/// an annualized rate via `total_return.powf(1/years) - 1`; `None` returns
/// the raw whole-period return.
pub fn twr(
    periods: &[(Money, Money)],
    annualize_over_years: Option<Decimal>,
) -> Result<Decimal, TwrError> {
    if periods.len() < 2 {
        return Err(TwrError::InsufficientPeriods);
    }

    let mut total_return = Decimal::ONE;
    for (i, window) in periods.windows(2).enumerate() {
        let (start_value, _) = window[0];
        let (end_value, cash_flow) = window[1];
        let adjusted_end = end_value - cash_flow;
        let period_return = adjusted_end
            .ratio(start_value)
            .ok_or(TwrError::ZeroStartingValue { period_index: i })?;
        total_return *= period_return;
    }

    match annualize_over_years {
        Some(years) => {
            let total_f64 = total_return.to_f64().unwrap_or(0.0);
            let years_f64 = years.to_f64().unwrap_or(1.0);
            let annualized = total_f64.powf(1.0 / years_f64) - 1.0;
            // A negative total-return base raised to a non-integer exponent
            // (any annualization period that isn't a whole number of
            // years) is undefined in real exponentiation - `powf` returns
            // `NaN`. The sign bit that NaN carries is platform/libm-
            // dependent, not portable, so `decimal_from_f64`'s
            // `is_sign_negative()` saturation guess can disagree across
            // platforms for the identical input. `.is_nan()` itself IS
            // portable (it checks the exponent/mantissa bit pattern, not
            // the sign bit), so detect NaN directly and pick the
            // saturation direction ourselves - a total-return multiplier
            // going negative is already a nonsensical, worse-than-total-
            // loss scenario, so it saturates to the very negative extreme
            // - rather than trust an incidental, non-portable NaN sign.
            if annualized.is_nan() {
                return Ok(Decimal::MIN);
            }
            Ok(super::irr::decimal_from_f64(annualized))
        }
        None => Ok(total_return - Decimal::ONE),
    }
}

/// TWR for a single holding, chained across its own appraisal history
/// (regardless of current status — a sold holding's recorded appraisal
/// history is still a valid, unambiguous performance record). Requires at
/// least 2 appraisals on record. Sub-period cash flows are this holding's
/// own transactions falling strictly after the previous appraisal and on
/// or before the current one.
pub fn holding_twr(
    repo: &Repository,
    holding_id: i64,
    annualize_over_years: Option<Decimal>,
) -> CardRoiResult<Decimal> {
    repo.get_holding(holding_id)?;
    let appraisals = repo.list_appraisals_for_holding(holding_id)?;
    if appraisals.len() < 2 {
        return Err(CardRoiError::validation(format!(
            "holding {holding_id} has {} appraisal(s) on record; time-weighted return needs at least 2 to form a sub-period",
            appraisals.len()
        )));
    }
    let transactions = repo.list_transactions_for_holding(holding_id)?;

    let periods = build_periods(&appraisals, |from, to| {
        transactions
            .iter()
            .filter(|t| t.transaction_date > from && t.transaction_date <= to)
            .fold(Money::ZERO, |acc, t| acc + twr_cash_flow(t))
    });

    twr(&periods, annualize_over_years).map_err(|e| CardRoiError::validation(e.to_string()))
}

/// TWR across the whole portfolio of *currently owned* holdings, chaining
/// sub-periods over the shared timeline formed by the union of every
/// owned holding's appraisal dates. At each timeline date, tracked value
/// is the sum of every owned holding's latest known appraisal as of that
/// date; a holding's first appraisal contributes its full value as a cash
/// flow (it just entered the tracked timeline, not a return) — the same
/// treatment a new position gets entering a tracked composite under GIPS.
///
/// Deliberately scoped to currently-owned holdings only: correctly
/// retiring a sold holding's value from a shared multi-asset timeline
/// (rather than either dropping it silently or leaving its last known
/// value counted forever) is real additional complexity with its own
/// failure modes; until that's built, a partial-but-correct number beats
/// a complete-but-subtly-wrong one.
/// Use `holding_twr` for a specific holding's full lifetime performance,
/// sold or not.
pub fn portfolio_twr(
    repo: &Repository,
    annualize_over_years: Option<Decimal>,
) -> CardRoiResult<Decimal> {
    let owned = repo.list_holdings(None, Some(HoldingStatus::Owned))?;

    struct History {
        appraisals: Vec<Appraisal>,
        transactions: Vec<Transaction>,
    }

    let mut histories = Vec::new();
    for holding in &owned {
        let appraisals = repo.list_appraisals_for_holding(holding.id)?;
        if appraisals.is_empty() {
            continue;
        }
        let transactions = repo.list_transactions_for_holding(holding.id)?;
        histories.push(History {
            appraisals,
            transactions,
        });
    }

    let mut timeline: Vec<NaiveDate> = histories
        .iter()
        .flat_map(|h| h.appraisals.iter().map(|a| a.appraised_date))
        .collect();
    timeline.sort();
    timeline.dedup();

    if timeline.len() < 2 {
        return Err(CardRoiError::validation(format!(
            "{} distinct appraisal date(s) across currently-owned holdings; time-weighted return needs at least 2 to form a sub-period",
            timeline.len()
        )));
    }

    fn value_as_of(h: &History, as_of: NaiveDate) -> Option<Money> {
        h.appraisals
            .iter()
            .rev()
            .find(|a| a.appraised_date <= as_of)
            .map(|a| a.appraised_value)
    }

    fn first_appraisal_date(h: &History) -> NaiveDate {
        h.appraisals[0].appraised_date
    }

    let portfolio_value_as_of = |as_of: NaiveDate| -> Money {
        histories
            .iter()
            .filter_map(|h| value_as_of(h, as_of))
            .fold(Money::ZERO, |acc, v| acc + v)
    };

    let mut periods = Vec::with_capacity(timeline.len());
    periods.push((portfolio_value_as_of(timeline[0]), Money::ZERO));

    for window in timeline.windows(2) {
        let (from, to) = (window[0], window[1]);
        let mut cash_flow = Money::ZERO;
        for h in &histories {
            if first_appraisal_date(h) <= from {
                cash_flow += h
                    .transactions
                    .iter()
                    .filter(|t| t.transaction_date > from && t.transaction_date <= to)
                    .fold(Money::ZERO, |acc, t| acc + twr_cash_flow(t));
            } else if let Some(entry_value) = value_as_of(h, to)
                .filter(|_| first_appraisal_date(h) > from && first_appraisal_date(h) <= to)
            {
                cash_flow += entry_value;
            }
        }
        periods.push((portfolio_value_as_of(to), cash_flow));
    }

    twr(&periods, annualize_over_years).map_err(|e| CardRoiError::validation(e.to_string()))
}

/// Builds `(value, cash_flow)` periods from a chronological appraisal list,
/// where `cash_flow_between(from, to)` computes the net external cash flow
/// for the window `(from, to]`.
fn build_periods(
    appraisals: &[Appraisal],
    cash_flow_between: impl Fn(NaiveDate, NaiveDate) -> Money,
) -> Vec<(Money, Money)> {
    let mut periods = Vec::with_capacity(appraisals.len());
    periods.push((appraisals[0].appraised_value, Money::ZERO));
    for window in appraisals.windows(2) {
        let cash_flow = cash_flow_between(window[0].appraised_date, window[1].appraised_date);
        periods.push((window[1].appraised_value, cash_flow));
    }
    periods
}

/// A contribution (money added to the tracked value) is positive; a
/// withdrawal is negative. This is the mirror image of `analytics::irr`'s
/// cash-flow sign convention, which is from the investor's wallet — here
/// the sign is from the tracked investment's own perspective.
fn twr_cash_flow(txn: &Transaction) -> Money {
    match txn.transaction_type {
        TransactionType::Acquisition | TransactionType::Adjustment => txn.total,
        TransactionType::Disposition => -txn.total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn money(s: &str) -> Money {
        s.parse().unwrap()
    }

    fn assert_close(actual: Decimal, expected: f64, tolerance: f64) {
        let diff = (actual.to_f64().unwrap() - expected).abs();
        assert!(
            diff < tolerance,
            "expected ~{expected}, got {actual} (diff {diff})"
        );
    }

    // --- pure-math cross-check against rust_finprim's rate::twr test
    // fixtures ---

    #[test]
    fn matches_rust_finprim_quarterly_reference_unannualized() {
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("1600.00"), money("400.00")),
            (money("1450.00"), -money("200.00")),
            (money("1700.00"), money("200.00")),
            (money("2200.00"), money("300.00")),
        ];
        let rate = twr(&periods, None).unwrap();
        assert_close(rate, 0.43078093, 1e-6);
    }

    #[test]
    fn matches_rust_finprim_quarterly_reference_annualized_over_4_years() {
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("1600.00"), money("400.00")),
            (money("1450.00"), -money("200.00")),
            (money("1700.00"), money("200.00")),
            (money("2200.00"), money("300.00")),
        ];
        let rate = twr(&periods, Some(Decimal::from(4))).unwrap();
        assert_close(rate, 0.093688, 1e-5);
    }

    #[test]
    fn matches_rust_finprim_six_quarter_reference_annualized_over_1_5_years() {
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("1600.00"), money("400.00")),
            (money("1450.00"), -money("200.00")),
            (money("1700.00"), money("200.00")),
            (money("2200.00"), money("300.00")),
            (money("2500.00"), Money::ZERO),
            (money("3000.00"), -money("300.00")),
        ];
        let rate = twr(&periods, Some(Decimal::new(15, 1))).unwrap();
        assert_close(rate, 0.663832, 1e-5);
    }

    #[test]
    fn seven_figure_portfolio_scale_matches_the_same_ratio_as_the_reference() {
        // Same exact shape/ratios as
        // matches_rust_finprim_quarterly_reference_unannualized, scaled
        // 5000x into six/seven-figure territory (the target user's actual
        // portfolio scale) - TWR is a pure ratio chain in exact Decimal
        // arithmetic, so the rate must come out identical, with no
        // precision loss or overflow from the larger cent counts.
        let periods = [
            (money("5000000.00"), Money::ZERO),
            (money("8000000.00"), money("2000000.00")),
            (money("7250000.00"), -money("1000000.00")),
            (money("8500000.00"), money("1000000.00")),
            (money("11000000.00"), money("1500000.00")),
        ];
        let rate = twr(&periods, None).unwrap();
        assert_close(rate, 0.43078093, 1e-6);
    }

    #[test]
    fn matches_rust_finprim_two_period_reference() {
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("1600.00"), money("400.00")),
        ];
        let rate = twr(&periods, None).unwrap();
        assert_close(rate, 0.2, 1e-9);
    }

    #[test]
    fn total_wipeout_as_the_final_period_is_exactly_negative_one() {
        // A final sub-period whose adjusted end value is exactly zero
        // zeroes out the whole chain - a complete loss reads as exactly
        // -100%, not a numerical artifact close to it. (Zero can only ever
        // be an *ending* value here, never a subsequent start - see
        // zero_starting_value_is_a_clear_error_not_a_panic for why a zero
        // mid-chain is an error, not silently 0% forever.)
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("500.00"), Money::ZERO),
            (money("0.00"), Money::ZERO),
        ];
        let rate = twr(&periods, None).unwrap();
        assert_eq!(rate, Decimal::from(-1));
    }

    #[test]
    fn negative_total_return_annualized_over_a_fractional_year_saturates_instead_of_zeroing() {
        // adjusted_end = 100 - 5000 = -4900; ratio -4900/1000 = -4.9, a
        // negative total-return base. Annualizing that over a fractional
        // year (1.5) raises a negative base to a fractional exponent -
        // undefined in real exponentiation, `NaN` in f64. A naive
        // `unwrap_or(Decimal::ZERO)` would report this as a deceptive 0%,
        // indistinguishable from "no change" - this test guards against
        // that regression.
        let periods = [
            (money("1000.00"), Money::ZERO),
            (money("100.00"), money("5000.00")),
        ];
        let rate = twr(&periods, Some(Decimal::new(15, 1))).unwrap();
        assert_ne!(rate, Decimal::ZERO);
        assert_eq!(rate, Decimal::MIN);
    }

    #[test]
    fn fewer_than_two_periods_is_insufficient() {
        assert_eq!(
            twr(&[(money("1000.00"), Money::ZERO)], None),
            Err(TwrError::InsufficientPeriods)
        );
        assert_eq!(twr(&[], None), Err(TwrError::InsufficientPeriods));
    }

    #[test]
    fn zero_starting_value_is_a_clear_error_not_a_panic() {
        let periods = [(Money::ZERO, Money::ZERO), (money("100.00"), Money::ZERO)];
        assert_eq!(
            twr(&periods, None),
            Err(TwrError::ZeroStartingValue { period_index: 0 })
        );
    }

    // --- repository-facing: holding_twr / portfolio_twr ---

    use crate::db::open_in_memory;
    use crate::models::{NewAppraisal, NewCard, NewHolding, NewSet, NewTransaction};

    fn repo() -> Repository {
        Repository::new(open_in_memory().unwrap())
    }

    fn seed_card(repo: &Repository, set_name: &str, card_number: &str) -> i64 {
        let set = repo
            .create_set(&NewSet {
                name: set_name.to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        repo.create_card(&NewCard {
            set_id: set.id,
            card_number: card_number.to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap()
        .id
    }

    #[test]
    fn holding_twr_chains_two_appraisals_with_no_transactions_between() {
        let repo = repo();
        let card_id = seed_card(&repo, "2023 Topps Chrome", "123");
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1000.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1200.00"),
            appraised_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_twr(&repo, holding.id, None).unwrap();
        // 1200/1000 - 1 = exactly 20%, no cash flow in between.
        assert_eq!(rate, Decimal::new(2, 1));
    }

    #[test]
    fn holding_twr_isolates_performance_from_an_adjustment_between_appraisals() {
        let repo = repo();
        let card_id = seed_card(&repo, "2023 Topps Chrome", "123");
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1000.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();
        // A $100 grading fee between the two appraisals - a contribution,
        // not performance. Without isolating it, the raw value change
        // (1300 - 1000)/1000 = 30% would overstate the true return.
        repo.create_transaction(&NewTransaction {
            holding_id: holding.id,
            transaction_type: TransactionType::Adjustment,
            price: money("100.00"),
            transaction_date: date(2026, 2, 1),
            ..Default::default()
        })
        .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1300.00"),
            appraised_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_twr(&repo, holding.id, None).unwrap();
        // adjusted_end = 1300 - 100 = 1200; 1200/1000 - 1 = 20%.
        assert_eq!(rate, Decimal::new(2, 1));
    }

    #[test]
    fn holding_twr_with_fewer_than_two_appraisals_is_a_clear_error() {
        let repo = repo();
        let card_id = seed_card(&repo, "2023 Topps Chrome", "123");
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    ..Default::default()
                },
            )
            .unwrap();

        let err = holding_twr(&repo, holding.id, None).unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }

    #[test]
    fn holding_twr_on_missing_holding_is_not_found() {
        let repo = repo();
        let err = holding_twr(&repo, 999, None).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn portfolio_twr_treats_a_new_holdings_first_appraisal_as_a_contribution_not_a_return() {
        let repo = repo();
        let card_a = seed_card(&repo, "2023 Topps Chrome", "123");
        let card_b = seed_card(&repo, "2024 Bowman", "45");

        let (holding_a, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: card_a,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_a.id,
            appraised_value: money("1000.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();

        // Holding B is bought and appraised for the first time on the
        // second timeline date - its full value must be treated as a
        // contribution entering the timeline, not portfolio growth.
        let (holding_b, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: card_b,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("500.00"),
                    transaction_date: date(2026, 4, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_b.id,
            appraised_value: money("500.00"),
            appraised_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();
        // Holding A also appreciates on the same second timeline date.
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_a.id,
            appraised_value: money("1200.00"),
            appraised_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = portfolio_twr(&repo, None).unwrap();
        // value_1 = 1000 (only A tracked so far)
        // value_2 = 1200 (A) + 500 (B, entering) = 1700
        // cash_flow_2 = 500 (B's full entry value)
        // adjusted_end = 1700 - 500 = 1200; 1200/1000 - 1 = 20%, matching
        // A's own appreciation exactly - B's entry must not distort it.
        assert_eq!(rate, Decimal::new(2, 1));
    }

    #[test]
    fn portfolio_twr_excludes_sold_holdings() {
        let repo = repo();
        let card_a = seed_card(&repo, "2023 Topps Chrome", "123");
        let card_b = seed_card(&repo, "2024 Bowman", "45");

        let (holding_a, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: card_a,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_a.id,
            appraised_value: money("1000.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_a.id,
            appraised_value: money("1200.00"),
            appraised_date: date(2026, 4, 1),
            ..Default::default()
        })
        .unwrap();

        // Holding B is appraised wildly high, then sold - it must not
        // pollute the currently-owned-only portfolio timeline at all.
        let (holding_b, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id: card_b,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("100.00"),
                    transaction_date: date(2026, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding_b.id,
            appraised_value: money("9999.00"),
            appraised_date: date(2026, 2, 1),
            ..Default::default()
        })
        .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: holding_b.id,
            price: money("9999.00"),
            transaction_date: date(2026, 3, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = portfolio_twr(&repo, None).unwrap();
        assert_eq!(rate, Decimal::new(2, 1));
    }

    #[test]
    fn portfolio_twr_with_fewer_than_two_distinct_dates_is_a_clear_error() {
        let repo = repo();
        let card_id = seed_card(&repo, "2023 Topps Chrome", "123");
        let (holding, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("1000.00"),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.create_appraisal(&NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1000.00"),
            appraised_date: date(2026, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let err = portfolio_twr(&repo, None).unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use super::*;

    // Strictly positive values only - zero-starting-value and
    // total-wipeout are real, already-covered error/edge cases
    // (see zero_starting_value_is_a_clear_error_not_a_panic and
    // total_wipeout_as_the_final_period_is_exactly_negative_one above),
    // not what this property is about.
    fn well_behaved_periods() -> impl Strategy<Value = Vec<(i64, i64)>> {
        prop::collection::vec((1i64..100_000_000, -1_000_000i64..1_000_000), 2..5)
    }

    proptest! {
        // Annualizing over exactly one year is mathematically an identity
        // (x^(1/1) - 1 == x - 1) - so the annualized and un-annualized
        // paths, despite going through genuinely different code (one
        // converts through f64 for the fractional-exponent power, the
        // other stays in exact Decimal the whole way), must agree for any
        // well-behaved period sequence, not just the specific reference
        // fixtures already hand-tested above.
        #[test]
        fn annualizing_over_exactly_one_year_is_identity(raw_periods in well_behaved_periods()) {
            let periods: Vec<(Money, Money)> = raw_periods
                .into_iter()
                .map(|(value, flow)| (Money::from_cents(value), Money::from_cents(flow)))
                .collect();

            let unannualized = twr(&periods, None).unwrap();
            let annualized_over_one_year = twr(&periods, Some(Decimal::ONE)).unwrap();

            let diff = (unannualized - annualized_over_one_year).abs();
            prop_assert!(
                diff < Decimal::new(1, 6),
                "unannualized {unannualized} vs annualized-over-1-year {annualized_over_one_year}, diff {diff}"
            );
        }
    }
}
