//! Exact monetary arithmetic.
//!
//! [`Money`] stores an amount as integer minor units (cents/pennies) so that
//! sums, differences, and comparisons across tens of thousands of
//! transactions never accumulate floating-point error. Parsing from decimal
//! strings goes through [`rust_decimal::Decimal`] so `"12.999"` is rejected
//! rather than silently truncated.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::str::FromStr;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CardRoiError;

/// An exact monetary amount, stored as integer cents.
///
/// Serializes as its formatted decimal string (`"525.00"`), not the raw
/// cent count — JSON/CSV output is read by humans and accounting tools,
/// not just round-tripped internally, so it must show dollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

impl Serialize for Money {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Money::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn abs(self) -> Self {
        Money(self.0.abs())
    }

    /// Returns `self / other` as a percentage-style ratio, or `None` if
    /// `other` is zero. Used for ROI calculations.
    pub fn ratio(self, other: Money) -> Option<Decimal> {
        if other.0 == 0 {
            return None;
        }
        Some(Decimal::from(self.0) / Decimal::from(other.0))
    }

    pub fn to_decimal(self) -> Decimal {
        Decimal::new(self.0, 2)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02}", abs / 100, abs % 100)
    }
}

impl FromStr for Money {
    type Err = CardRoiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_start_matches('$');
        let without_commas =
            strip_thousands_separators(trimmed).ok_or_else(|| CardRoiError::InvalidMoney {
                raw: s.to_string(),
                reason: "ambiguous comma placement - use a plain number (\"10.00\") or \
                         standard thousands grouping (\"1,234.56\"); a comma is never a \
                         decimal point here"
                    .to_string(),
            })?;
        let decimal =
            Decimal::from_str(&without_commas).map_err(|e| CardRoiError::InvalidMoney {
                raw: s.to_string(),
                reason: e.to_string(),
            })?;
        Self::try_from(decimal)
    }
}

/// Removes valid US-style thousands-grouping commas from the *integer*
/// part of a decimal string (e.g. `"1,234,567.89"` -> `"1234567.89"`),
/// returning `None` if any comma is placed ambiguously — most importantly
/// a lone two-digit group like `"10,00"`, which is how many locales write
/// ten (dollars/euros) using a comma as the *decimal* point. Blindly
/// stripping every comma (a naive `.replace(',', "")`) would silently turn
/// that into 1000.00, a 100x error with no warning; this only accepts
/// comma placement that is unambiguously a thousands grouping.
fn strip_thousands_separators(s: &str) -> Option<String> {
    let (sign, unsigned) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    if !unsigned.contains(',') {
        return Some(s.to_string());
    }

    let (int_part, frac_part) = match unsigned.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (unsigned, None),
    };
    if frac_part.is_some_and(|f| f.contains(',')) {
        return None; // a comma after the decimal point is never valid
    }

    let groups: Vec<&str> = int_part.split(',').collect();
    let [first, rest @ ..] = groups.as_slice() else {
        return None;
    };
    let is_digits = |g: &str| !g.is_empty() && g.chars().all(|c| c.is_ascii_digit());
    if !(1..=3).contains(&first.len()) || !is_digits(first) {
        return None;
    }
    if !rest.iter().all(|g| g.len() == 3 && is_digits(g)) {
        return None;
    }

    let joined_int = groups.concat();
    Some(match frac_part {
        Some(f) => format!("{sign}{joined_int}.{f}"),
        None => format!("{sign}{joined_int}"),
    })
}

impl TryFrom<Decimal> for Money {
    type Error = CardRoiError;

    fn try_from(decimal: Decimal) -> Result<Self, Self::Error> {
        let scaled = decimal * Decimal::new(100, 0);
        if scaled.fract() != Decimal::ZERO {
            return Err(CardRoiError::InvalidMoney {
                raw: decimal.to_string(),
                reason: "amount has sub-cent precision".to_string(),
            });
        }
        let cents = scaled.to_i64().ok_or_else(|| CardRoiError::InvalidMoney {
            raw: decimal.to_string(),
            reason: "amount out of range".to_string(),
        })?;
        Ok(Money(cents))
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Self) -> Self::Output {
        Money(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Self) -> Self::Output {
        Money(self.0 - rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Self::Output {
        Money(-self.0)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Self {
        iter.fold(Money::ZERO, Add::add)
    }
}

impl rusqlite::types::ToSql for Money {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(self.0))
    }
}

impl rusqlite::types::FromSql for Money {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(Money)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_decimal() {
        assert_eq!(Money::from_str("12.99").unwrap(), Money::from_cents(1299));
    }

    #[test]
    fn parses_dollar_sign_and_commas() {
        assert_eq!(
            Money::from_str("$1,234.50").unwrap(),
            Money::from_cents(123450)
        );
    }

    #[test]
    fn parses_multiple_thousands_groups() {
        assert_eq!(
            Money::from_str("1,234,567.89").unwrap(),
            Money::from_cents(123456789)
        );
    }

    #[test]
    fn parses_thousands_grouped_integer_with_no_decimal() {
        assert_eq!(Money::from_str("1,000").unwrap(), Money::from_cents(100000));
    }

    #[test]
    fn rejects_european_style_comma_decimal_as_ambiguous() {
        // "10,00" is how many locales write ten dollars/euros. Silently
        // treating the comma as a thousands separator (as a naive
        // .replace(',', "") would) turns it into 1000.00 - a 100x error
        // with no warning. Must be rejected outright, not guessed at.
        let err = Money::from_str("10,00").unwrap_err();
        assert!(matches!(err, CardRoiError::InvalidMoney { .. }));
    }

    #[test]
    fn rejects_a_two_digit_first_thousands_group_as_ambiguous() {
        // "1,23" isn't valid US grouping (the first group before further
        // groups must be exactly 3 digits) and isn't valid European
        // decimal notation either (that would need exactly 2 digits after
        // the comma with no thousands grouping context) - reject rather
        // than guess.
        assert!(Money::from_str("1,23").is_err());
    }

    #[test]
    fn rejects_a_short_non_first_thousands_group() {
        assert!(Money::from_str("12,3.45").is_err());
    }

    #[test]
    fn rejects_a_long_first_thousands_group() {
        assert!(Money::from_str("1234,567.89").is_err());
    }

    #[test]
    fn rejects_comma_inside_the_fractional_part() {
        assert!(Money::from_str("12.3,4").is_err());
    }

    #[test]
    fn rejects_trailing_comma() {
        assert!(Money::from_str("1,000,").is_err());
    }

    #[test]
    fn parses_negative_thousands_grouped_amount() {
        assert_eq!(
            Money::from_str("-1,234.50").unwrap(),
            -Money::from_cents(123450)
        );
    }

    #[test]
    fn rejects_sub_cent_precision() {
        assert!(Money::from_str("12.999").is_err());
    }

    #[test]
    fn display_formats_two_decimals() {
        assert_eq!(Money::from_cents(1299).to_string(), "12.99");
        assert_eq!(Money::from_cents(5).to_string(), "0.05");
        assert_eq!(Money::from_cents(-1299).to_string(), "-12.99");
    }

    #[test]
    fn arithmetic_is_exact() {
        let a = Money::from_str("0.10").unwrap();
        let b = Money::from_str("0.20").unwrap();
        assert_eq!(a + b, Money::from_cents(30));
    }

    #[test]
    fn serializes_to_json_as_a_formatted_decimal_string_not_raw_cents() {
        let json = serde_json::to_string(&Money::from_cents(52500)).unwrap();
        assert_eq!(
            json, "\"525.00\"",
            "a report opened by a human/accountant must show dollars, not internal cent counts"
        );
    }

    #[test]
    fn deserializes_from_json_decimal_string() {
        let money: Money = serde_json::from_str("\"525.00\"").unwrap();
        assert_eq!(money, Money::from_cents(52500));
    }

    #[test]
    fn json_round_trip_preserves_negative_amounts() {
        let original = -Money::from_str("12.34").unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    #[should_panic]
    fn addition_overflow_panics_instead_of_wrapping() {
        // Only proves something under `cargo test --release`, where
        // [profile.release]'s overflow-checks = true (Cargo.toml) is in
        // effect. Under the default dev/test profile, debug_assertions
        // already turns overflow-checks on regardless of this setting, so
        // this test alone can't distinguish "the profile flag matters" from
        // "debug builds always check" - it's the release run that matters.
        let _ = Money::from_cents(i64::MAX) + Money::from_cents(1);
    }
}

#[cfg(test)]
mod proptests {
    use std::str::FromStr;

    use proptest::prelude::*;

    use super::Money;

    // Bounded well below i64::MAX so addition/subtraction in these
    // properties can't overflow - overflow behavior itself is covered by
    // `addition_overflow_panics_instead_of_wrapping` above, not the point
    // of these tests. This range still comfortably covers any real
    // portfolio (a hundred trillion dollars in cents).
    const REALISTIC_CENTS: std::ops::RangeInclusive<i64> = -10_000_000_000_000..=10_000_000_000_000;

    proptest! {
        // Every amount CardROI stores must survive a display/parse round
        // trip exactly - this is the property the "money is never a
        // float, never silently rounds" guarantee actually rests on.
        #[test]
        fn display_then_parse_round_trips_exactly(cents in REALISTIC_CENTS) {
            let money = Money::from_cents(cents);
            let rendered = money.to_string();
            let reparsed = Money::from_str(&rendered).unwrap();
            prop_assert_eq!(reparsed, money);
        }

        // Addition and subtraction must be exact inverses - no
        // representable amount should be able to "leak" or "gain" a cent
        // through arithmetic.
        #[test]
        fn add_then_subtract_is_identity(a in REALISTIC_CENTS, b in REALISTIC_CENTS) {
            let a = Money::from_cents(a);
            let b = Money::from_cents(b);
            prop_assert_eq!((a + b) - b, a);
        }

        // Money can only ever be constructed at cent granularity - so
        // round-tripping through a decimal string can never introduce or
        // lose fractional cents, for any amount in the realistic range.
        #[test]
        fn from_cents_to_decimal_string_has_exactly_two_decimal_places(cents in REALISTIC_CENTS) {
            let rendered = Money::from_cents(cents).to_string();
            let decimal_part = rendered.trim_start_matches('-').split('.').nth(1);
            prop_assert_eq!(decimal_part.map(str::len), Some(2));
        }
    }
}
