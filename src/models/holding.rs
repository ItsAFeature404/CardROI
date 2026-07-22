//! The [`Holding`] domain model — a specific physical copy of a [`Card`](super::card::Card)
//! that is or was owned. Each row is exactly one unique item; graded and
//! serial-numbered cards cannot be fungibly merged with other copies.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CardRoiError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoldingStatus {
    Owned,
    Sold,
    Lost,
    Damaged,
}

impl HoldingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldingStatus::Owned => "owned",
            HoldingStatus::Sold => "sold",
            HoldingStatus::Lost => "lost",
            HoldingStatus::Damaged => "damaged",
        }
    }

    /// True for any terminal status backed by a real disposition
    /// transaction (a sale, or a recorded loss via
    /// [`crate::db::repository::Repository::record_loss`]) — as opposed to
    /// `Owned`, the only non-terminal status.
    pub fn is_closed(self) -> bool {
        !matches!(self, HoldingStatus::Owned)
    }
}

impl std::str::FromStr for HoldingStatus {
    type Err = CardRoiError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "owned" => Ok(HoldingStatus::Owned),
            "sold" => Ok(HoldingStatus::Sold),
            "lost" => Ok(HoldingStatus::Lost),
            "damaged" => Ok(HoldingStatus::Damaged),
            other => Err(CardRoiError::validation(format!(
                "unknown holding status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
    pub id: i64,
    pub card_id: i64,
    pub serial_number: Option<String>,
    pub grade: Option<String>,
    pub grading_company: Option<String>,
    pub cert_number: Option<String>,
    pub status: HoldingStatus,
    pub acquired_date: Option<NaiveDate>,
    pub disposed_date: Option<NaiveDate>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewHolding {
    pub card_id: i64,
    pub serial_number: Option<String>,
    pub grade: Option<String>,
    pub grading_company: Option<String>,
    pub cert_number: Option<String>,
    pub acquired_date: Option<NaiveDate>,
    pub notes: Option<String>,
}

impl NewHolding {
    pub fn validate(&self) -> Result<()> {
        if self.card_id <= 0 {
            return Err(CardRoiError::validation(
                "holding must reference a valid card",
            ));
        }
        if self.grade.is_some() && self.grading_company.is_none() {
            return Err(CardRoiError::validation(
                "grading_company is required when grade is set",
            ));
        }
        Ok(())
    }
}

/// The editable subset of a holding's own attributes - physical/grading
/// details a collector can genuinely mistype and need to correct later.
/// Deliberately excludes `card_id` (moving a holding to a different card
/// isn't "fixing a typo," it's a different holding entirely - delete and
/// re-buy instead), `status`/`disposed_date` (governed by `record_sale`/
/// `record_loss`, which also write the matching disposition transaction -
/// editing status here would desync it from the ledger), and
/// `acquired_date` (the acquisition transaction's own `transaction_date`
/// is the actual source of truth for when a holding was bought; edit that
/// via `update_transaction` instead of maintaining two dates that could
/// disagree).
#[derive(Debug, Clone, Default)]
pub struct HoldingEdit {
    pub serial_number: Option<String>,
    pub grade: Option<String>,
    pub grading_company: Option<String>,
    pub cert_number: Option<String>,
    pub notes: Option<String>,
}

impl HoldingEdit {
    pub fn validate(&self) -> Result<()> {
        if self.grade.is_some() && self.grading_company.is_none() {
            return Err(CardRoiError::validation(
                "grading_company is required when grade is set",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_holding() -> NewHolding {
        NewHolding {
            card_id: 1,
            serial_number: Some("12/25".to_string()),
            grade: Some("10".to_string()),
            grading_company: Some("PSA".to_string()),
            cert_number: Some("123456".to_string()),
            acquired_date: None,
            notes: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_holding() {
        assert!(valid_holding().validate().is_ok());
    }

    #[test]
    fn accepts_an_ungraded_holding() {
        let holding = NewHolding {
            grade: None,
            grading_company: None,
            cert_number: None,
            ..valid_holding()
        };
        assert!(holding.validate().is_ok());
    }

    #[test]
    fn rejects_non_positive_card_id() {
        let holding = NewHolding {
            card_id: 0,
            ..valid_holding()
        };
        assert!(holding.validate().is_err());
    }

    #[test]
    fn rejects_grade_without_grading_company() {
        let holding = NewHolding {
            grading_company: None,
            ..valid_holding()
        };
        assert!(holding.validate().is_err());
    }

    #[test]
    fn holding_status_round_trips_through_str() {
        for status in [
            HoldingStatus::Owned,
            HoldingStatus::Sold,
            HoldingStatus::Lost,
            HoldingStatus::Damaged,
        ] {
            let parsed: HoldingStatus = status.as_str().parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn holding_status_rejects_unknown_string() {
        assert!("unknown".parse::<HoldingStatus>().is_err());
    }

    #[test]
    fn holding_edit_accepts_an_ungraded_edit() {
        let edit = HoldingEdit {
            serial_number: Some("12/25".to_string()),
            ..Default::default()
        };
        assert!(edit.validate().is_ok());
    }

    #[test]
    fn holding_edit_rejects_grade_without_grading_company() {
        let edit = HoldingEdit {
            grade: Some("10".to_string()),
            grading_company: None,
            ..Default::default()
        };
        assert!(edit.validate().is_err());
    }
}
