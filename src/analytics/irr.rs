//! XIRR (Internal Rate of Return for irregular, dated cash flows).
//!
//! Pure math — no `Repository`, no DB — so it's testable in complete
//! isolation from the rest of the financial engine.
//!
//! Cash flow `Money` amounts convert to `f64` only at the point of calling
//! the root-finder below; the public API takes/returns `Money`/`Decimal`.
//! This is a deliberate, narrow exception to "money is never `f64`": an
//! IRR is inherently the root of a transcendental equation found by
//! iterative approximation — that's what "tolerance" means. What must
//! stay exact is the *stored* ledger; the *derived rate* is allowed to be
//! an approximation because there's no other kind. Excel's own XIRR works
//! the same way internally.
//!
//! Day-count convention: 365-day year, matching the XIRR convention used
//! by Excel, LibreOffice, and other standard reference implementations.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::db::repository::Repository;
use crate::error::{CardRoiError, Result as CardRoiResult};
use crate::models::{HoldingStatus, Money, TransactionType};

const DAYS_PER_YEAR: f64 = 365.0;
const DEFAULT_GUESS: f64 = 0.1;
const TOLERANCE: f64 = 1e-9;
const MAX_NEWTON_ITER: u32 = 50;
const MAX_BISECTION_ITER: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrrError {
    /// Fewer than 2 cash flows — no rate of return is defined.
    InsufficientCashFlows,
    /// All cash flows have the same sign, so no discount rate can make
    /// NPV zero — there is no real IRR, not a solver failure.
    NoSignChange,
    /// Newton-Raphson and the bisection fallback both failed to converge
    /// within the iteration budget.
    DidNotConverge {
        last_rate: Decimal,
        residual_npv: Decimal,
    },
}

/// Solves for the annualized internal rate of return of a series of dated
/// cash flows. Outflows (money spent) are negative, inflows (money
/// received) are positive. The first entry's date is the epoch every
/// other cash flow is measured against; order of the remaining entries
/// does not matter.
pub fn xirr(cash_flows: &[(NaiveDate, Money)]) -> Result<Decimal, IrrError> {
    if cash_flows.len() < 2 {
        return Err(IrrError::InsufficientCashFlows);
    }

    let flows = to_year_fraction_flows(cash_flows);

    if !has_sign_change(&flows) {
        return Err(IrrError::NoSignChange);
    }

    match solve_rate(&flows) {
        Ok(rate) => Ok(decimal_from_f64(rate)),
        Err(last_rate) => Err(IrrError::DidNotConverge {
            last_rate: decimal_from_f64(last_rate),
            residual_npv: decimal_from_f64(xnpv(last_rate, &flows)),
        }),
    }
}

/// Runs Newton-Raphson, falling back to bisection: `Ok(rate)` is the raw
/// f64 rate that was verified (internally, at full f64 precision) to
/// satisfy `|NPV(rate)| < TOLERANCE`; `Err(last_attempted_rate)` carries
/// the best estimate reached if neither method converged. Kept
/// `pub(crate)` so tests can verify the solver's actual root-finding
/// correctness directly, without going through `decimal_from_f64`'s
/// intentional precision reduction - Decimal only has ~17 significant
/// digits by design (to avoid exposing f64 binary-representation noise).
///
/// This distinction is load-bearing, not pedantic: near a root where
/// |NPV'(rate)| is large (which happens here as rate approaches the -1
/// domain boundary, or at very large positive rates - both are regions
/// where (1+rate)^t is extremely steep), the classical root-conditioning
/// result (Wilkinson's polynomial; see Trefethen & Bau, "Numerical Linear
/// Algebra", Lecture 12, and Higham, "Accuracy and Stability of Numerical
/// Algorithms", 2nd ed.) says a root's sensitivity to input perturbation
/// scales with 1/|f'(root)| - so an input shift as small as Decimal's own
/// rounding (~1e-13 in the last significant digit, confirmed by direct
/// measurement) can move a recomputed NPV by thousands, even though the
/// solver's raw f64 arithmetic found an exact root. Verifying against the
/// rounded public Decimal instead of this raw value would be testing a
/// different, much more fragile property than "did the solver work."
pub(crate) fn solve_rate(flows: &[(f64, f64)]) -> Result<f64, f64> {
    match newton_raphson(flows, DEFAULT_GUESS) {
        Ok(rate) => Ok(rate),
        Err(last) => bisection(flows).ok_or(last),
    }
}

impl std::fmt::Display for IrrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrrError::InsufficientCashFlows => {
                write!(f, "at least 2 cash flows are required to compute an IRR")
            }
            IrrError::NoSignChange => {
                write!(f, "no real IRR exists (all cash flows have the same sign)")
            }
            IrrError::DidNotConverge {
                last_rate,
                residual_npv,
            } => write!(
                f,
                "IRR did not converge (last estimate {last_rate}, residual NPV {residual_npv})"
            ),
        }
    }
}

/// IRR for a single holding. A *closed* holding (sold, lost, or damaged)
/// already has a real disposition transaction as its terminal cash flow —
/// no appraisal needed, the disposition already ends the story. A
/// still-*owned* holding instead uses its latest appraisal as the terminal
/// inflow on the appraisal's date, per standard practice for illiquid
/// assets (the GIPS / private-equity residual-NAV convention: the
/// unrealized residual value is added as a final cash flow on the
/// valuation date). An owned holding with no appraisal has no terminal
/// value and returns an error rather than a fabricated one.
pub fn holding_irr(repo: &Repository, holding_id: i64) -> CardRoiResult<Decimal> {
    let holding = repo.get_holding(holding_id)?;
    let mut flows = holding_cash_flows(repo, holding_id)?;

    if holding.status == HoldingStatus::Owned {
        let appraisal = repo
            .latest_appraisal_for_holding(holding_id)?
            .ok_or_else(|| {
                CardRoiError::validation(format!(
                    "holding {holding_id} is not yet sold and has no appraisal; IRR needs a terminal cash flow (a sale, or an appraisal for open positions)"
                ))
            })?;
        flows.push((appraisal.appraised_date, appraisal.appraised_value));
        flows.sort_by_key(|(date, _)| *date);
    }
    // Any other status (Sold/Lost/Damaged) is closed and already has a real
    // disposition transaction in `flows` — see Repository::record_sale and
    // record_loss.

    xirr(&flows).map_err(|e| CardRoiError::validation(e.to_string()))
}

/// Portfolio-level IRR across every closed holding's (sold, lost, or
/// damaged) cash flows combined into a single XIRR. Open (still-owned)
/// holdings are excluded.
pub fn portfolio_irr_closed_positions(repo: &Repository) -> CardRoiResult<Decimal> {
    let closed: Vec<_> = repo
        .list_holdings(None, None)?
        .into_iter()
        .filter(|h| h.status.is_closed())
        .collect();
    let mut flows = Vec::new();
    for holding in &closed {
        flows.extend(holding_cash_flows(repo, holding.id)?);
    }
    if flows.is_empty() {
        return Err(CardRoiError::validation(
            "no closed (sold/lost/damaged) holdings to compute portfolio IRR from",
        ));
    }
    flows.sort_by_key(|(date, _)| *date);
    xirr(&flows).map_err(|e| CardRoiError::validation(e.to_string()))
}

fn holding_cash_flows(
    repo: &Repository,
    holding_id: i64,
) -> CardRoiResult<Vec<(NaiveDate, Money)>> {
    let transactions = repo.list_transactions_for_holding(holding_id)?;
    Ok(transactions_to_cash_flows(&transactions))
}

/// Maps a holding's transactions to XIRR cash flows (outflow for
/// acquisitions/adjustments, inflow for dispositions) with no repository
/// access — `pub(crate)` so `analytics::whatif` can reuse it on real
/// transactions it already fetched, then append one hypothetical
/// disposition, without a parallel cash-flow-building path.
pub(crate) fn transactions_to_cash_flows(
    transactions: &[crate::models::Transaction],
) -> Vec<(NaiveDate, Money)> {
    transactions
        .iter()
        .map(|txn| {
            let amount = match txn.transaction_type {
                TransactionType::Acquisition | TransactionType::Adjustment => -txn.total,
                TransactionType::Disposition => txn.total,
            };
            (txn.transaction_date, amount)
        })
        .collect()
}

fn money_to_f64(amount: Money) -> f64 {
    amount.cents() as f64 / 100.0
}

/// Converts dated cash flows into the `(years_from_first_flow, amount)`
/// pairs the root-finders operate on. `pub(crate)` (in addition to being
/// used by `xirr` itself) so tests can independently recompute NPV at a
/// solved rate without duplicating this date-to-year-fraction logic.
pub(crate) fn to_year_fraction_flows(cash_flows: &[(NaiveDate, Money)]) -> Vec<(f64, f64)> {
    let base_date = cash_flows[0].0;
    cash_flows
        .iter()
        .map(|(date, amount)| {
            let years = (*date - base_date).num_days() as f64 / DAYS_PER_YEAR;
            (years, money_to_f64(*amount))
        })
        .collect()
}

/// Converts an f64 to `Decimal`, saturating to a signed extreme when the
/// value can't be represented (non-finite, e.g. an overflowed residual NPV
/// at a wildly out-of-domain rate, or a magnitude/precision beyond
/// `Decimal`'s range) rather than silently falling back to zero. A silent
/// zero here would be indistinguishable from "no residual" i.e.
/// "converged" - discovered via a `DidNotConverge` test whose extreme
/// cash-flow ratio produced exactly this: an infinite residual NPV that
/// `unwrap_or(Decimal::ZERO)` reported as a deceptive `0`.
pub(crate) fn decimal_from_f64(value: f64) -> Decimal {
    match Decimal::try_from(value) {
        Ok(d) => d,
        Err(_) => {
            if value.is_sign_negative() {
                Decimal::MIN
            } else {
                Decimal::MAX
            }
        }
    }
}

fn has_sign_change(flows: &[(f64, f64)]) -> bool {
    let mut saw_positive = false;
    let mut saw_negative = false;
    for &(_, cf) in flows {
        if cf > 0.0 {
            saw_positive = true;
        }
        if cf < 0.0 {
            saw_negative = true;
        }
    }
    saw_positive && saw_negative
}

/// Net present value of `flows` (each `(years_from_epoch, amount)`) at
/// `rate`, under the 365-day-year XIRR convention.
fn xnpv(rate: f64, flows: &[(f64, f64)]) -> f64 {
    flows.iter().map(|&(t, cf)| cf / (1.0 + rate).powf(t)).sum()
}

fn xnpv_prime(rate: f64, flows: &[(f64, f64)]) -> f64 {
    flows
        .iter()
        .map(|&(t, cf)| -t * cf / (1.0 + rate).powf(t + 1.0))
        .sum()
}

/// Returns `Ok(rate)` on convergence, `Err(last_attempted_rate)` if it
/// exhausts `MAX_NEWTON_ITER` or walks outside the valid domain
/// (`rate <= -1.0` makes `(1+rate)^t` undefined for non-integer `t`).
fn newton_raphson(flows: &[(f64, f64)], guess: f64) -> Result<f64, f64> {
    let mut rate = guess;
    for _ in 0..MAX_NEWTON_ITER {
        if rate <= -1.0 {
            return Err(rate);
        }
        let f = xnpv(rate, flows);
        if f.abs() < TOLERANCE {
            return Ok(rate);
        }
        let f_prime = xnpv_prime(rate, flows);
        if f_prime.abs() < 1e-12 {
            return Err(rate); // derivative too flat, Newton step would blow up
        }
        let next_rate = rate - f / f_prime;
        if !next_rate.is_finite() {
            return Err(rate);
        }
        rate = next_rate;
    }
    Err(rate)
}

/// Fallback for cash-flow patterns Newton-Raphson can't converge on (e.g.
/// multiple sign changes producing a non-monotonic NPV curve). Scans a
/// wide range of candidate rates for a sign change in NPV, then bisects
/// within the first bracket found.
fn bisection(flows: &[(f64, f64)]) -> Option<f64> {
    let mut candidates = vec![-0.99, -0.9, -0.75, -0.5, -0.25, 0.0];
    let mut r = 0.1;
    while r <= 100.0 {
        candidates.push(r);
        r *= 1.5;
    }

    let mut bracket = None;
    let mut prev = candidates[0];
    let mut prev_val = xnpv(prev, flows);
    for &c in &candidates[1..] {
        let val = xnpv(c, flows);
        if prev_val.is_finite() && val.is_finite() && prev_val.signum() != val.signum() {
            bracket = Some((prev, c));
            break;
        }
        prev = c;
        prev_val = val;
    }

    let (mut lo, mut hi) = bracket?;
    let mut f_lo = xnpv(lo, flows);

    for _ in 0..MAX_BISECTION_ITER {
        let mid = (lo + hi) / 2.0;
        let f_mid = xnpv(mid, flows);
        if f_mid.abs() < TOLERANCE {
            return Some(mid);
        }
        // Deliberately does NOT also accept a narrow bracket width
        // (`hi - lo` small) as convergence on its own: near the rate=-1
        // domain boundary, `xnpv` can be extremely steep, so the bracket
        // can shrink to a tiny width in rate-space while the NPV at that
        // point is still enormous - a real bug a property test caught
        // (a converged rate whose own NPV was in the hundreds of
        // millions, not ~0). A narrow bracket with no small residual
        // means this candidate root isn't numerically well-conditioned,
        // not that it's found - falling through to `None`/DidNotConverge
        // is the honest outcome, not a silently wrong rate.
        if !(hi - lo).abs().is_normal() {
            break;
        }
        if f_lo.signum() == f_mid.signum() {
            lo = mid;
            f_lo = f_mid;
        } else {
            hi = mid;
        }
    }
    None
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

    /// Asserts the computed rate matches an independently-verified
    /// reference value (cross-checked via scipy.optimize.brentq, a
    /// well-established root-finder, not our own solver) to within 1e-6.
    fn assert_close(actual: Decimal, expected: f64) {
        let diff = (actual - Decimal::try_from(expected).unwrap()).abs();
        assert!(
            diff < Decimal::new(1, 6),
            "expected ~{expected}, got {actual} (diff {diff})"
        );
    }

    #[test]
    fn simple_buy_sell_exactly_one_year_apart_is_exact_ten_percent() {
        // -1000 on day 0, +1100 exactly 365 days later (2023 is not a leap
        // year, so this is exactly t=1.0 under the 365-day convention):
        // -1000 + 1100/(1+r)^1 = 0  =>  r = 0.10 exactly.
        let flows = [
            (date(2023, 1, 1), -money("1000.00")),
            (date(2024, 1, 1), money("1100.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn near_domain_boundary_rate_never_silently_reports_a_wrong_root() {
        // Found by analytics::irr::proptests::converged_rate_is_always_a_
        // real_root_of_npv on Windows CI: bisection's old "bracket narrow
        // => converged" shortcut accepted a rate of ~-0.99 whose actual
        // NPV was 258 million, nowhere near zero. Near the rate=-1 domain
        // boundary xnpv is extremely steep, so the bracket can shrink to a
        // tiny width in rate-space long before the residual is actually
        // small. Locked in as a permanent regression case.
        use chrono::Days;
        let base = date(2020, 1, 1);
        let flows = [
            (
                base.checked_add_days(Days::new(195)).unwrap(),
                Money::from_cents(1),
            ),
            (
                base.checked_add_days(Days::new(2905)).unwrap(),
                Money::from_cents(-8_061_961),
            ),
            (
                base.checked_add_days(Days::new(160)).unwrap(),
                Money::from_cents(-51_567),
            ),
            (
                base.checked_add_days(Days::new(2990)).unwrap(),
                Money::from_cents(2_758_591),
            ),
        ];

        match xirr(&flows) {
            Ok(rate) => {
                let year_fraction_flows = to_year_fraction_flows(&flows);
                let npv = xnpv(rate.to_string().parse().unwrap(), &year_fraction_flows);
                assert!(
                    npv.abs() < 1e-2,
                    "converged to {rate} but NPV there is {npv}, not ~0"
                );
            }
            Err(IrrError::DidNotConverge { .. }) => {
                // Also an acceptable, honest outcome for this numerically
                // unstable input - the point is it must never be a
                // silently wrong "converged" rate.
            }
            Err(other) => panic!("expected Ok or DidNotConverge, got {other:?}"),
        }
    }

    #[test]
    fn multiple_partial_cash_flows() {
        let flows = [
            (date(2023, 1, 1), -money("1000.00")),
            (date(2023, 7, 1), -money("500.00")),
            (date(2024, 1, 1), money("1800.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.2422268678497653);
    }

    #[test]
    fn irregular_day_count_gaps() {
        let flows = [
            (date(2023, 1, 1), -money("1000.00")),
            (date(2023, 3, 17), -money("200.00")),
            (date(2023, 11, 2), money("1500.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.32071479181179136);
    }

    #[test]
    fn multiple_sign_changes_converges_via_bisection_fallback() {
        // This pattern (- + - +) is exactly the class of case plain
        // Newton-Raphson can fail to converge on (multiple candidate
        // IRRs / a non-monotonic NPV curve) - why a bisection fallback
        // exists at all, not just Newton-Raphson alone.
        let flows = [
            (date(2023, 1, 1), -money("1000.00")),
            (date(2023, 6, 1), money("2000.00")),
            (date(2023, 12, 1), -money("1200.00")),
            (date(2024, 6, 1), money("500.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 1.3867549133655683);
    }

    #[test]
    fn all_outflows_has_no_real_irr() {
        let flows = [
            (date(2023, 1, 1), -money("100.00")),
            (date(2023, 6, 1), -money("50.00")),
        ];
        assert_eq!(xirr(&flows), Err(IrrError::NoSignChange));
    }

    #[test]
    fn all_inflows_has_no_real_irr() {
        let flows = [
            (date(2023, 1, 1), money("100.00")),
            (date(2023, 6, 1), money("50.00")),
        ];
        assert_eq!(xirr(&flows), Err(IrrError::NoSignChange));
    }

    #[test]
    fn single_cash_flow_is_insufficient() {
        let flows = [(date(2023, 1, 1), money("100.00"))];
        assert_eq!(xirr(&flows), Err(IrrError::InsufficientCashFlows));
    }

    #[test]
    fn seven_figure_portfolio_scale_is_exact_ten_percent() {
        // Same exact-10%-over-one-year shape as
        // simple_buy_sell_exactly_one_year_apart_is_exact_ten_percent, just
        // scaled to the six/seven-figure single-card and portfolio values
        // this app's target user ("The Operator") actually deals in -
        // confirms no precision loss or overflow at that scale.
        let flows = [
            (date(2023, 1, 1), -money("5000000.00")),
            (date(2024, 1, 1), money("5500000.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.10000000000000009);
    }

    #[test]
    fn large_amount_over_a_single_day_does_not_lose_precision() {
        // A very short holding period (1 day) combined with a large,
        // realistic seven-figure amount - stresses the 365-day-year
        // division for a tiny numerator without overflowing or rounding
        // away the result.
        let flows = [
            (date(2023, 1, 1), -money("1000000.00")),
            (date(2023, 1, 2), money("1000027.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.009903586069891501);
    }

    #[test]
    fn very_long_holding_period_computes_correctly() {
        // 40 calendar years (14,610 actual days, including leap years), a
        // modest amount - stresses the opposite extreme from the 1-day test
        // above (a very large `t` in `(1+rate)^t`). Expected value computed
        // independently against the actual calendar day count, not a naive
        // 40*365, since the XIRR convention divides by the real day gap.
        let flows = [
            (date(1984, 1, 1), -money("10000.00")),
            (date(2024, 1, 1), money("20000.00")),
        ];
        let rate = xirr(&flows).unwrap();
        assert_close(rate, 0.017467624015804706);
    }

    #[test]
    fn decimal_from_f64_saturates_non_finite_values_instead_of_reporting_zero() {
        assert_eq!(decimal_from_f64(f64::INFINITY), Decimal::MAX);
        assert_eq!(decimal_from_f64(f64::NEG_INFINITY), Decimal::MIN);
        // A real, representable value must still convert normally - the
        // fix only changes behavior for values Decimal can't represent.
        assert_eq!(decimal_from_f64(0.1), Decimal::try_from(0.1).unwrap());
    }

    #[test]
    fn empty_cash_flows_is_insufficient() {
        let flows: [(NaiveDate, Money); 0] = [];
        assert_eq!(xirr(&flows), Err(IrrError::InsufficientCashFlows));
    }

    // --- repository-facing: holding_irr / portfolio_irr_closed_positions ---

    use crate::db::open_in_memory;
    use crate::models::{NewCard, NewHolding, NewSet, NewTransaction};

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

    #[test]
    fn holding_irr_matches_exact_ten_percent_reference() {
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
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("1100.00"),
            transaction_date: date(2024, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_irr(&repo, holding.id).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn holding_irr_on_still_owned_holding_is_a_clear_error() {
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
                    ..Default::default()
                },
            )
            .unwrap();

        let err = holding_irr(&repo, holding.id).unwrap_err();
        assert!(err.to_string().contains("not yet sold"));
    }

    #[test]
    fn holding_irr_on_open_position_uses_latest_appraisal_as_terminal_value() {
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
        // Same exact-10% cash-flow shape as the closed-position reference
        // test, but via an appraisal instead of an actual sale.
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1100.00"),
            appraised_date: date(2024, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_irr(&repo, holding.id).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn holding_irr_on_open_position_uses_latest_appraisal_by_date_not_insertion_order() {
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
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("1100.00"),
            appraised_date: date(2024, 1, 1),
            ..Default::default()
        })
        .unwrap();
        // Inserted after, but dated earlier - the stale one must not win.
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("9999.00"),
            appraised_date: date(2023, 6, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_irr(&repo, holding.id).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn holding_irr_prefers_the_real_sale_over_a_stale_appraisal() {
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
        // A wildly different appraisal on file from before the sale - once
        // sold, the real disposition proceeds must win, not this estimate.
        repo.create_appraisal(&crate::models::NewAppraisal {
            holding_id: holding.id,
            appraised_value: money("5000.00"),
            appraised_date: date(2023, 6, 1),
            ..Default::default()
        })
        .unwrap();
        repo.record_sale(NewTransaction {
            holding_id: holding.id,
            price: money("1100.00"),
            transaction_date: date(2024, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let rate = holding_irr(&repo, holding.id).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn portfolio_irr_aggregates_multiple_sold_holdings() {
        let repo = repo();
        let card_id = seed_card(&repo);

        // Same exact 10% round trip as the single-holding test, twice over -
        // combining two identical cash-flow patterns must still solve to
        // the same 10% (a portfolio of two identical bets is that bet).
        for _ in 0..2 {
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
            repo.record_sale(NewTransaction {
                holding_id: holding.id,
                price: money("1100.00"),
                transaction_date: date(2024, 1, 1),
                ..Default::default()
            })
            .unwrap();
        }

        let rate = portfolio_irr_closed_positions(&repo).unwrap();
        assert_close(rate, 0.10000000000002678);
    }

    #[test]
    fn portfolio_irr_with_no_sold_holdings_is_a_clear_error() {
        let repo = repo();
        let card_id = seed_card(&repo);
        repo.record_acquisition(
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

        let err = portfolio_irr_closed_positions(&repo).unwrap_err();
        assert!(err.to_string().contains("no closed"));
    }

    #[test]
    fn holding_irr_on_a_total_loss_has_no_real_solution_and_errors_clearly() {
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
        repo.record_loss(
            holding.id,
            HoldingStatus::Lost,
            date(2024, 1, 1),
            Money::ZERO,
            Money::ZERO,
            None,
            None,
        )
        .unwrap();

        // NPV(rate) = -1000 + 0 is a nonzero constant for every rate - there
        // is genuinely no real root, so this must be the same clear "no real
        // IRR exists" error as any other same-sign cash-flow set, never a
        // fabricated -100%. (roi::holding_pnl still correctly reports the
        // -100% realized loss - IRR and realized P&L are different
        // questions, and IRR is undefined here while P&L is not.)
        let err = holding_irr(&repo, holding.id).unwrap_err();
        assert!(err.to_string().contains("no real IRR exists"));
    }

    #[test]
    fn holding_irr_on_a_partial_loss_computes_the_exact_negative_rate() {
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
        repo.record_loss(
            holding.id,
            HoldingStatus::Damaged,
            date(2024, 1, 1),
            money("100.00"),
            Money::ZERO,
            Some("crease".to_string()),
            None,
        )
        .unwrap();

        // -1000 at t=0, +100 at t=1 year exactly: NPV(r) = -1000 + 100/(1+r)
        // = 0 => r = -0.9 exactly.
        let rate = holding_irr(&repo, holding.id).unwrap();
        assert_close(rate, -0.9);
    }

    #[test]
    fn portfolio_irr_includes_a_lost_holding_alongside_a_sold_one() {
        let repo = repo();
        let card_id = seed_card(&repo);

        let (sold, _) = repo
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
        repo.record_sale(NewTransaction {
            holding_id: sold.id,
            price: money("1100.00"),
            transaction_date: date(2024, 1, 1),
            ..Default::default()
        })
        .unwrap();

        let (lost, _) = repo
            .record_acquisition(
                &NewHolding {
                    card_id,
                    ..Default::default()
                },
                NewTransaction {
                    price: money("500.00"),
                    transaction_date: date(2023, 1, 1),
                    ..Default::default()
                },
            )
            .unwrap();
        repo.record_loss(
            lost.id,
            HoldingStatus::Lost,
            date(2024, 1, 1),
            Money::ZERO,
            Money::ZERO,
            None,
            None,
        )
        .unwrap();

        // Before the fix, portfolio_irr_closed_positions only ever queried
        // Sold holdings, so the lost holding's -500 outflow (with no
        // offsetting inflow) would have been silently excluded from the
        // portfolio's IRR entirely.
        let with_lost_holding = portfolio_irr_closed_positions(&repo).unwrap();
        let sold_only = xirr(&[
            (date(2023, 1, 1), -money("1000.00")),
            (date(2024, 1, 1), money("1100.00")),
        ])
        .unwrap();
        assert!(
            with_lost_holding < sold_only,
            "including a total-loss holding must pull portfolio IRR down from the sold-only rate"
        );
    }

    #[test]
    fn extreme_cash_flow_ratio_reports_did_not_converge_instead_of_a_bogus_rate() {
        // A massive outflow followed one day later by a negligible inflow
        // has a true root below -0.99 (Newton's domain floor) - both
        // Newton-Raphson and bisection's candidate scan (which starts at
        // -0.99) fail to find it. This must surface as DidNotConverge with
        // useful diagnostic data, never a silently wrong rate.
        let flows = vec![
            (date(2026, 1, 1), -money("100000000.00")),
            (date(2026, 1, 2), money("0.01")),
        ];

        let err = xirr(&flows).unwrap_err();

        match err {
            IrrError::DidNotConverge {
                last_rate,
                residual_npv,
            } => {
                // A near-zero residual would mean it actually converged,
                // contradicting DidNotConverge - the diagnostic data must be
                // real evidence of failure, not a disguised success.
                assert_ne!(
                    residual_npv,
                    Decimal::ZERO,
                    "a DidNotConverge with zero residual NPV would actually mean it converged"
                );
                assert!(
                    last_rate <= Decimal::from(-1) || last_rate >= Decimal::ZERO,
                    "expected the solver to have given up outside the well-behaved (-1, 0) range, got {last_rate}"
                );
            }
            other => panic!("expected DidNotConverge, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod proptests {
    use chrono::Days;
    use proptest::prelude::*;

    use super::*;

    // 2-6 (day_offset, cents) pairs. day_offset is bounded to ~10 years so
    // date arithmetic can't overflow; cents deliberately excludes 0 (a
    // zero-amount cash flow isn't a meaningful contribution either way).
    fn cash_flow_series() -> impl Strategy<Value = Vec<(u32, i64)>> {
        prop::collection::vec(
            (
                0u32..3650,
                (-10_000_000i64..10_000_000).prop_filter("nonzero", |c| *c != 0),
            ),
            2..6,
        )
    }

    proptest! {
        // Whenever the solver reports success, its raw f64 rate must be a
        // real root: plugging it back into the NPV formula must land back
        // at (approximately) zero. This is the property the whole solver
        // exists to guarantee - a "converged" rate that doesn't satisfy
        // its own NPV equation would be silently wrong, exactly the kind
        // of bug hand-picked reference-value tests could miss if they
        // happened not to exercise the specific cash-flow shape that
        // triggers it.
        //
        // Deliberately tests `solve_rate` (the raw f64 result) rather than
        // the public `xirr` (which returns a `Decimal`): converting to
        // Decimal for the public API is an intentional, separate
        // precision reduction (~17 significant digits, so callers never
        // see f64 binary-representation noise) that has nothing to do
        // with whether the *solver* found a real root. Very close to an
        // ill-conditioned root - confirmed by direct measurement near
        // rate=-1 and at large positive rates, where (1+rate)^t is
        // extremely steep - that Decimal rounding alone (a ~1e-13 shift in
        // the last significant digit) can move a recomputed NPV by
        // thousands, even though the solver's own f64 arithmetic was
        // exactly right. That's a property of how any fixed-precision
        // representation behaves near a singularity, not a defect in the
        // root-finding being tested here - so this test verifies the
        // solver directly, at its own working precision, instead of
        // conflating it with that separate concern.
        #[test]
        fn solved_rate_is_always_a_real_root_of_npv(raw_flows in cash_flow_series()) {
            let base = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
            let flows: Vec<(NaiveDate, Money)> = raw_flows
                .into_iter()
                .map(|(day_offset, cents)| {
                    (base.checked_add_days(Days::new(day_offset as u64)).unwrap(), Money::from_cents(cents))
                })
                .collect();
            let year_fraction_flows = to_year_fraction_flows(&flows);

            if !has_sign_change(&year_fraction_flows) {
                // No real IRR can exist - nothing to solve or verify.
                return Ok(());
            }

            if let Ok(rate) = solve_rate(&year_fraction_flows) {
                let npv = xnpv(rate, &year_fraction_flows);
                prop_assert!(
                    npv.abs() < TOLERANCE,
                    "solve_rate returned {rate} but NPV at that rate is {npv}, not < TOLERANCE ({TOLERANCE})"
                );
            }
            // Err is a legitimate outcome (the solver's own convergence
            // budget genuinely can't be satisfied for some cash-flow
            // shapes) - nothing to assert in that case.
        }
    }
}
