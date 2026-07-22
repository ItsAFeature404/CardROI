//! The [`Transaction`] domain model — a single ledger entry against a
//! [`Holding`](super::holding::Holding): an acquisition, a disposition, or a
//! cost-basis adjustment (e.g. a later grading fee).

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CardRoiError, Result};
use crate::models::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    /// A purchase or other cost-basis-establishing acquisition.
    Acquisition,
    /// A sale or other disposal that generates proceeds.
    Disposition,
    /// A post-acquisition cost-basis change (e.g. a grading submission fee).
    Adjustment,
}

impl TransactionType {
    pub fn as_str(self) -> &'static str {
        match self {
            TransactionType::Acquisition => "acquisition",
            TransactionType::Disposition => "disposition",
            TransactionType::Adjustment => "adjustment",
        }
    }
}

impl std::str::FromStr for TransactionType {
    type Err = CardRoiError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "acquisition" => Ok(TransactionType::Acquisition),
            "disposition" => Ok(TransactionType::Disposition),
            "adjustment" => Ok(TransactionType::Adjustment),
            other => Err(CardRoiError::validation(format!(
                "unknown transaction type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub holding_id: i64,
    pub transaction_type: TransactionType,
    pub transaction_date: NaiveDate,
    pub price: Money,
    pub fees: Money,
    pub shipping: Money,
    pub tax: Money,
    pub other_cost: Money,
    pub total: Money,
    pub currency: String,
    pub counterparty: Option<String>,
    pub platform: Option<String>,
    pub external_ref: Option<String>,
    pub notes: Option<String>,
    /// Populated only on the `Disposition` transaction a lost/damaged event
    /// creates (see [`crate::db::repository::Repository::record_loss`]) —
    /// salvage/market value the holding retains, `None`/zero for a total
    /// loss. Kept distinct from `insurance_recovery` per IRS Pub. 547:
    /// salvage value retained and insurance reimbursement received are
    /// both subtracted from cost basis, but they come from different
    /// sources and shouldn't be conflated into one figure.
    pub residual_value: Option<Money>,
    /// Insurance/other reimbursement received for a loss event, distinct
    /// from `residual_value` — see the doc comment there.
    pub insurance_recovery: Option<Money>,
    /// Free-text cause of a loss event (e.g. "water damage", "theft").
    /// Informational/tax-substantiation only; never used in any P&L,
    /// ROI, IRR, or TWR calculation.
    pub loss_cause: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTransaction {
    pub holding_id: i64,
    pub transaction_type: TransactionType,
    pub transaction_date: NaiveDate,
    pub price: Money,
    pub fees: Money,
    pub shipping: Money,
    pub tax: Money,
    pub other_cost: Money,
    pub currency: String,
    pub counterparty: Option<String>,
    pub platform: Option<String>,
    pub external_ref: Option<String>,
    pub notes: Option<String>,
    pub residual_value: Option<Money>,
    pub insurance_recovery: Option<Money>,
    pub loss_cause: Option<String>,
}

impl Default for NewTransaction {
    fn default() -> Self {
        Self {
            holding_id: 0,
            transaction_type: TransactionType::Acquisition,
            transaction_date: Utc::now().date_naive(),
            price: Money::ZERO,
            fees: Money::ZERO,
            shipping: Money::ZERO,
            tax: Money::ZERO,
            other_cost: Money::ZERO,
            currency: "USD".to_string(),
            counterparty: None,
            platform: None,
            external_ref: None,
            notes: None,
            residual_value: None,
            insurance_recovery: None,
            loss_cause: None,
        }
    }
}

impl NewTransaction {
    pub fn validate(&self) -> Result<()> {
        if self.holding_id <= 0 {
            return Err(CardRoiError::validation(
                "transaction must reference a valid holding",
            ));
        }
        if self.price.is_negative() {
            return Err(CardRoiError::validation("price cannot be negative"));
        }
        for (label, amount) in [
            ("fees", self.fees),
            ("shipping", self.shipping),
            ("tax", self.tax),
            ("other_cost", self.other_cost),
        ] {
            if amount.is_negative() {
                return Err(CardRoiError::validation(format!(
                    "{label} cannot be negative"
                )));
            }
        }
        if self.currency.trim().is_empty() {
            return Err(CardRoiError::validation("currency must not be empty"));
        }
        for (label, amount) in [
            ("residual_value", self.residual_value),
            ("insurance_recovery", self.insurance_recovery),
        ] {
            if amount.is_some_and(|a| a.is_negative()) {
                return Err(CardRoiError::validation(format!(
                    "{label} cannot be negative"
                )));
            }
        }
        Ok(())
    }

    /// The total effect on cash flow for this transaction:
    /// - Acquisitions and adjustments add to cost basis: `price + fees + shipping + tax + other_cost`.
    /// - Dispositions net proceeds against selling costs: `price - fees - shipping - tax - other_cost`.
    pub fn total(&self) -> Money {
        let costs = self.fees + self.shipping + self.tax + self.other_cost;
        match self.transaction_type {
            TransactionType::Acquisition | TransactionType::Adjustment => self.price + costs,
            TransactionType::Disposition => self.price - costs,
        }
    }
}

/// The editable subset of an existing transaction's fields - a data-entry
/// correction surface (wrong price, wrong date, a typo), not a way to
/// reclassify a ledger entry. Deliberately excludes `transaction_type` and
/// `holding_id` (changing either would silently reclassify or move a
/// ledger entry rather than fix a typo on the one it already is) and
/// `residual_value`/`insurance_recovery`/`loss_cause` (set only by
/// `record_loss`, not a generic edit surface - editing a loss's numbers
/// after the fact belongs to a dedicated loss-editing flow this project
/// doesn't have yet, not this one).
#[derive(Debug, Clone)]
pub struct TransactionEdit {
    pub transaction_date: NaiveDate,
    pub price: Money,
    pub fees: Money,
    pub shipping: Money,
    pub tax: Money,
    pub other_cost: Money,
    pub currency: String,
    pub counterparty: Option<String>,
    pub platform: Option<String>,
    pub external_ref: Option<String>,
    pub notes: Option<String>,
}

impl TransactionEdit {
    pub fn validate(&self) -> Result<()> {
        if self.price.is_negative() {
            return Err(CardRoiError::validation("price cannot be negative"));
        }
        for (label, amount) in [
            ("fees", self.fees),
            ("shipping", self.shipping),
            ("tax", self.tax),
            ("other_cost", self.other_cost),
        ] {
            if amount.is_negative() {
                return Err(CardRoiError::validation(format!(
                    "{label} cannot be negative"
                )));
            }
        }
        if self.currency.trim().is_empty() {
            return Err(CardRoiError::validation("currency must not be empty"));
        }
        Ok(())
    }

    /// Same formula as `NewTransaction::total()`, parameterized on the
    /// existing transaction's own (immutable) type rather than one this
    /// struct carries - see the doc comment above for why type isn't
    /// editable here.
    pub(crate) fn total(&self, transaction_type: TransactionType) -> Money {
        let costs = self.fees + self.shipping + self.tax + self.other_cost;
        match transaction_type {
            TransactionType::Acquisition | TransactionType::Adjustment => self.price + costs,
            TransactionType::Disposition => self.price - costs,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn base_txn() -> NewTransaction {
        NewTransaction {
            holding_id: 1,
            price: Money::from_str("100.00").unwrap(),
            fees: Money::from_str("5.00").unwrap(),
            shipping: Money::from_str("3.00").unwrap(),
            tax: Money::from_str("2.00").unwrap(),
            other_cost: Money::ZERO,
            ..Default::default()
        }
    }

    #[test]
    fn acquisition_total_adds_all_costs_to_price() {
        let txn = NewTransaction {
            transaction_type: TransactionType::Acquisition,
            ..base_txn()
        };
        assert_eq!(txn.total(), Money::from_str("110.00").unwrap());
    }

    #[test]
    fn adjustment_total_adds_all_costs_to_price() {
        let txn = NewTransaction {
            transaction_type: TransactionType::Adjustment,
            ..base_txn()
        };
        assert_eq!(txn.total(), Money::from_str("110.00").unwrap());
    }

    #[test]
    fn disposition_total_nets_costs_against_price() {
        let txn = NewTransaction {
            transaction_type: TransactionType::Disposition,
            ..base_txn()
        };
        assert_eq!(txn.total(), Money::from_str("90.00").unwrap());
    }

    #[test]
    fn rejects_non_positive_holding_id() {
        let txn = NewTransaction {
            holding_id: 0,
            ..base_txn()
        };
        assert!(txn.validate().is_err());
    }

    #[test]
    fn rejects_negative_price() {
        let txn = NewTransaction {
            price: -Money::from_str("1.00").unwrap(),
            ..base_txn()
        };
        assert!(txn.validate().is_err());
    }

    #[test]
    fn rejects_negative_fees() {
        let txn = NewTransaction {
            fees: -Money::from_str("1.00").unwrap(),
            ..base_txn()
        };
        assert!(txn.validate().is_err());
    }

    #[test]
    fn rejects_empty_currency() {
        let txn = NewTransaction {
            currency: "".to_string(),
            ..base_txn()
        };
        assert!(txn.validate().is_err());
    }

    #[test]
    fn transaction_type_round_trips_through_str() {
        for kind in [
            TransactionType::Acquisition,
            TransactionType::Disposition,
            TransactionType::Adjustment,
        ] {
            let parsed: TransactionType = kind.as_str().parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn transaction_type_rejects_unknown_string() {
        assert!("unknown".parse::<TransactionType>().is_err());
    }

    fn base_edit() -> TransactionEdit {
        TransactionEdit {
            transaction_date: Utc::now().date_naive(),
            price: Money::from_str("100.00").unwrap(),
            fees: Money::from_str("5.00").unwrap(),
            shipping: Money::from_str("3.00").unwrap(),
            tax: Money::from_str("2.00").unwrap(),
            other_cost: Money::ZERO,
            currency: "USD".to_string(),
            counterparty: None,
            platform: None,
            external_ref: None,
            notes: None,
        }
    }

    #[test]
    fn transaction_edit_total_matches_new_transactions_formula() {
        assert_eq!(
            base_edit().total(TransactionType::Acquisition),
            Money::from_str("110.00").unwrap()
        );
        assert_eq!(
            base_edit().total(TransactionType::Disposition),
            Money::from_str("90.00").unwrap()
        );
    }

    #[test]
    fn transaction_edit_rejects_negative_price() {
        let edit = TransactionEdit {
            price: -Money::from_str("1.00").unwrap(),
            ..base_edit()
        };
        assert!(edit.validate().is_err());
    }

    #[test]
    fn transaction_edit_rejects_empty_currency() {
        let edit = TransactionEdit {
            currency: "".to_string(),
            ..base_edit()
        };
        assert!(edit.validate().is_err());
    }
}
