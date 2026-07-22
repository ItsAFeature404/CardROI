//! The [`Set`] domain model — a product/set such as "2023 Topps Chrome".

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CardRoiError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Set {
    pub id: i64,
    pub name: String,
    pub sport: String,
    pub year: Option<i32>,
    pub brand: Option<String>,
    pub total_cards: Option<i32>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields required to create a new [`Set`]. Separated from `Set` itself so
/// callers can't construct a fully-formed record (with an id and timestamps)
/// out of thin air.
#[derive(Debug, Clone, Default)]
pub struct NewSet {
    pub name: String,
    pub sport: String,
    pub year: Option<i32>,
    pub brand: Option<String>,
    pub total_cards: Option<i32>,
    pub notes: Option<String>,
}

impl NewSet {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(CardRoiError::validation("set name must not be empty"));
        }
        if self.sport.trim().is_empty() {
            return Err(CardRoiError::validation("set sport must not be empty"));
        }
        if let Some(year) = self.year
            && !(1800..=2200).contains(&year)
        {
            return Err(CardRoiError::validation(format!(
                "set year {year} is out of plausible range"
            )));
        }
        if let Some(total) = self.total_cards
            && total <= 0
        {
            return Err(CardRoiError::validation("set total_cards must be positive"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_set() -> NewSet {
        NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            year: Some(2023),
            brand: Some("Topps".to_string()),
            total_cards: Some(220),
            notes: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_set() {
        assert!(valid_set().validate().is_ok());
    }

    #[test]
    fn rejects_empty_name() {
        let set = NewSet {
            name: "   ".to_string(),
            ..valid_set()
        };
        assert!(set.validate().is_err());
    }

    #[test]
    fn rejects_empty_sport() {
        let set = NewSet {
            sport: "".to_string(),
            ..valid_set()
        };
        assert!(set.validate().is_err());
    }

    #[test]
    fn rejects_implausible_year() {
        let set = NewSet {
            year: Some(1500),
            ..valid_set()
        };
        assert!(set.validate().is_err());
    }

    #[test]
    fn rejects_non_positive_total_cards() {
        let set = NewSet {
            total_cards: Some(0),
            ..valid_set()
        };
        assert!(set.validate().is_err());
    }

    #[test]
    fn allows_missing_optional_fields() {
        let set = NewSet {
            year: None,
            brand: None,
            total_cards: None,
            notes: None,
            ..valid_set()
        };
        assert!(set.validate().is_ok());
    }
}
