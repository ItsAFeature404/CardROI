//! The [`Card`] domain model — a catalog entry within a [`Set`](super::set::Set),
//! e.g. "2023 Topps Chrome #123 LeBron James Refractor". Not a specific
//! physical copy; see [`crate::models::holding::Holding`] for that.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CardRoiError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: i64,
    pub set_id: i64,
    pub card_number: String,
    pub player_name: String,
    pub variant: Option<String>,
    pub parallel_name: Option<String>,
    pub print_run: Option<i32>,
    pub is_rookie: bool,
    pub is_autograph: bool,
    pub is_relic: bool,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewCard {
    pub set_id: i64,
    pub card_number: String,
    pub player_name: String,
    pub variant: Option<String>,
    pub parallel_name: Option<String>,
    pub print_run: Option<i32>,
    pub is_rookie: bool,
    pub is_autograph: bool,
    pub is_relic: bool,
    pub notes: Option<String>,
}

impl NewCard {
    pub fn validate(&self) -> Result<()> {
        if self.set_id <= 0 {
            return Err(CardRoiError::validation("card must reference a valid set"));
        }
        if self.card_number.trim().is_empty() {
            return Err(CardRoiError::validation("card_number must not be empty"));
        }
        if self.player_name.trim().is_empty() {
            return Err(CardRoiError::validation("player_name must not be empty"));
        }
        if let Some(run) = self.print_run
            && run <= 0
        {
            return Err(CardRoiError::validation("print_run must be positive"));
        }
        Ok(())
    }
}

impl Card {
    /// Human-readable identity for display in tables and reports, e.g.
    /// `"LeBron James #123 (Refractor, Gold /25)"` - player name leads,
    /// matching how a real listing/collector actually identifies a card
    /// (by who's on it first), with the card number as a secondary tag
    /// rather than a leading identifier.
    pub fn display_name(&self) -> String {
        let mut name = format!("{} #{}", self.player_name, self.card_number);
        let mut descriptors = Vec::new();
        if let Some(v) = &self.variant {
            descriptors.push(v.clone());
        }
        if let Some(p) = &self.parallel_name {
            match self.print_run {
                Some(run) => descriptors.push(format!("{p} /{run}")),
                None => descriptors.push(p.clone()),
            }
        }
        if !descriptors.is_empty() {
            name.push_str(&format!(" ({})", descriptors.join(", ")));
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_card() -> NewCard {
        NewCard {
            set_id: 1,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            variant: Some("Refractor".to_string()),
            parallel_name: Some("Gold".to_string()),
            print_run: Some(25),
            is_rookie: false,
            is_autograph: false,
            is_relic: false,
            notes: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_card() {
        assert!(valid_card().validate().is_ok());
    }

    #[test]
    fn rejects_non_positive_set_id() {
        let card = NewCard {
            set_id: 0,
            ..valid_card()
        };
        assert!(card.validate().is_err());
    }

    #[test]
    fn rejects_empty_card_number() {
        let card = NewCard {
            card_number: "".to_string(),
            ..valid_card()
        };
        assert!(card.validate().is_err());
    }

    #[test]
    fn rejects_empty_player_name() {
        let card = NewCard {
            player_name: "  ".to_string(),
            ..valid_card()
        };
        assert!(card.validate().is_err());
    }

    #[test]
    fn rejects_non_positive_print_run() {
        let card = NewCard {
            print_run: Some(0),
            ..valid_card()
        };
        assert!(card.validate().is_err());
    }

    #[test]
    fn display_name_includes_variant_and_numbered_parallel() {
        let card = Card {
            id: 1,
            set_id: 1,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            variant: Some("Refractor".to_string()),
            parallel_name: Some("Gold".to_string()),
            print_run: Some(25),
            is_rookie: false,
            is_autograph: false,
            is_relic: false,
            notes: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(
            card.display_name(),
            "LeBron James #123 (Refractor, Gold /25)"
        );
    }

    #[test]
    fn display_name_omits_parens_when_no_descriptors() {
        let card = Card {
            id: 1,
            set_id: 1,
            card_number: "1".to_string(),
            player_name: "Base Card".to_string(),
            variant: None,
            parallel_name: None,
            print_run: None,
            is_rookie: false,
            is_autograph: false,
            is_relic: false,
            notes: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(card.display_name(), "Base Card #1");
    }
}
