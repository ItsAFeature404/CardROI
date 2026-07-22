//! The [`Appraisal`] domain model — a manual, timestamped valuation the
//! user types in themselves for a [`Holding`](super::holding::Holding).
//! Never derived from a market-price feed.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CardRoiError, Result};
use crate::models::money::Money;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appraisal {
    pub id: i64,
    pub holding_id: i64,
    pub appraised_value: Money,
    pub appraised_date: NaiveDate,
    pub source: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAppraisal {
    pub holding_id: i64,
    pub appraised_value: Money,
    pub appraised_date: NaiveDate,
    pub source: Option<String>,
    pub notes: Option<String>,
}

impl Default for NewAppraisal {
    fn default() -> Self {
        Self {
            holding_id: 0,
            appraised_value: Money::ZERO,
            appraised_date: Utc::now().date_naive(),
            source: None,
            notes: None,
        }
    }
}

impl NewAppraisal {
    pub fn validate(&self) -> Result<()> {
        if self.holding_id <= 0 {
            return Err(CardRoiError::validation(
                "appraisal must reference a valid holding",
            ));
        }
        if self.appraised_value.is_negative() {
            return Err(CardRoiError::validation(
                "appraised_value cannot be negative",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn valid_appraisal() -> NewAppraisal {
        NewAppraisal {
            holding_id: 1,
            appraised_value: Money::from_str("500.00").unwrap(),
            appraised_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            source: Some("PSA pop report comp".to_string()),
            notes: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_appraisal() {
        assert!(valid_appraisal().validate().is_ok());
    }

    #[test]
    fn accepts_a_zero_value_appraisal() {
        let appraisal = NewAppraisal {
            appraised_value: Money::ZERO,
            ..valid_appraisal()
        };
        assert!(appraisal.validate().is_ok());
    }

    #[test]
    fn rejects_non_positive_holding_id() {
        let appraisal = NewAppraisal {
            holding_id: 0,
            ..valid_appraisal()
        };
        assert!(appraisal.validate().is_err());
    }

    #[test]
    fn rejects_negative_appraised_value() {
        let appraisal = NewAppraisal {
            appraised_value: -Money::from_str("1.00").unwrap(),
            ..valid_appraisal()
        };
        assert!(appraisal.validate().is_err());
    }
}
