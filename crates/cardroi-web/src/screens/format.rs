//! Shared display formatting for financial figures across screens.
//! `Money`'s own `Display` impl is deliberately plain (no currency
//! symbol, no digit grouping) since it also backs the CLI's text
//! output; the GUI's presentation layer adds both here so every screen
//! formats money identically instead of drifting screen by screen.

use cardroi::models::Money;
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// "$1,234,019.35" / "-$42.00". CardROI has no multi-currency handling
/// anywhere yet (`Transaction::currency` is recorded but never surfaced
/// in any rollup), so this doesn't attempt it either.
pub fn money(amount: Money) -> String {
    let cents = amount.cents();
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let (dollars, remainder) = (abs / 100, abs % 100);
    format!("{sign}${}.{remainder:02}", group_thousands(dollars))
}

fn group_thousands(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut groups = Vec::new();
    while n > 0 {
        groups.push(n % 1000);
        n /= 1000;
    }
    let most_significant = groups.pop().expect("checked non-empty above");
    let mut out = most_significant.to_string();
    for group in groups.into_iter().rev() {
        out.push_str(&format!(",{group:03}"));
    }
    out
}

/// "12.40%" - a ratio (e.g. `Money::ratio`'s output) as a percentage.
pub fn percent(ratio: Decimal) -> String {
    // `Decimal`'s `{:.2}` formatting truncates toward zero rather than
    // rounding (verified directly against the CLI's own identical bug in
    // `commands::roi::as_percent` - see that fix for the full reasoning)
    // - `round_dp` must run first.
    format!("{:.2}%", (ratio * Decimal::from(100)).round_dp(2))
}

/// "07-19-2026" - the CLI/CSV/JSON interchange layer keeps ISO 8601
/// (`YYYY-MM-DD`, unambiguous for scripting and interop), but this app's
/// GUI is US-audience-only for now, where MM-DD-YYYY is the familiar
/// everyday format - display and manual entry both use it here, distinct
/// from the CLI's own format.
pub fn date(d: NaiveDate) -> String {
    d.format("%m-%d-%Y").to_string()
}

/// The manual-entry counterpart to `date` above - every Date `FormField`
/// across every form in this crate parses through this one function
/// instead of each screen re-deriving its own `NaiveDate` parsing, so the
/// accepted format can never drift screen by screen.
pub fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s.trim(), "%m-%d-%Y")
        .map_err(|_| format!("invalid date {s:?}, expected MM-DD-YYYY"))
}

/// "3 years, 4 months" / "5 months" / "12 days" - deliberately coarse
/// (days become months past 60, months become years past 365) since a
/// collector reads "three years" faster than "1,186 days." The years
/// threshold matters: past a year, "13 months" reads worse than "1
/// year, 1 month" - caught by a test expecting the latter at 400 days.
/// Lives here rather than privately in `holding_detail.rs` (its one
/// current caller) since duration formatting is a general need other
/// screens - Reports, most likely - are the next to want.
pub fn duration_phrase(days: i64) -> String {
    if days < 60 {
        return match days {
            0 => "less than a day".to_string(),
            1 => "1 day".to_string(),
            n => format!("{n} days"),
        };
    }
    if days < 365 {
        return match days / 30 {
            1 => "1 month".to_string(),
            n => format!("{n} months"),
        };
    }
    let years = days / 365;
    let year_part = match years {
        1 => "1 year".to_string(),
        n => format!("{n} years"),
    };
    match (days % 365) / 30 {
        0 => year_part,
        1 => format!("{year_part}, 1 month"),
        n => format!("{year_part}, {n} months"),
    }
}

// No `jpeg_data_uri` here - card-photo capture is explicitly out of
// scope for a browser tab, so pulling in `base64` for a function nothing
// here would ever call isn't warranted.

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn formats_with_grouping_and_sign() {
        assert_eq!(money(Money::from_cents(123_401_935)), "$1,234,019.35");
        assert_eq!(money(Money::from_cents(-4200)), "-$42.00");
        assert_eq!(money(Money::ZERO), "$0.00");
        assert_eq!(money(Money::from_cents(500)), "$5.00");
        assert_eq!(money(Money::from_cents(100_000)), "$1,000.00");
    }

    #[wasm_bindgen_test]
    fn formats_percent_to_two_decimal_places() {
        assert_eq!(percent(Decimal::new(1240, 4)), "12.40%");
    }

    #[wasm_bindgen_test]
    fn date_formats_as_mm_dd_yyyy_not_the_clis_iso_format() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 19).unwrap();
        assert_eq!(date(d), "07-19-2026");
    }

    #[wasm_bindgen_test]
    fn parse_date_accepts_mm_dd_yyyy_and_rejects_the_clis_iso_format() {
        assert_eq!(
            parse_date("07-19-2026"),
            Ok(NaiveDate::from_ymd_opt(2026, 7, 19).unwrap())
        );
        // The CLI's own ISO format must not be silently accepted too -
        // one unambiguous format per surface, not "either works here."
        assert!(parse_date("2026-07-19").is_err());
        assert!(parse_date("not a date").is_err());
    }

    #[wasm_bindgen_test]
    fn rounds_half_up_instead_of_truncating() {
        // 23/63 = 0.36507936...%, which formatting-by-truncation would
        // wrongly render as "36.50%" instead of the correctly-rounded
        // "36.51%" - this exact ratio comes from a real -$115/$315
        // unrealized-loss cross-check against the CLI.
        let ratio = Decimal::from(23) / Decimal::from(63);
        assert_eq!(percent(ratio), "36.51%");
        assert_eq!(percent(-ratio), "-36.51%");
    }

    #[wasm_bindgen_test]
    fn duration_phrase_is_coarse_and_reads_naturally() {
        assert_eq!(duration_phrase(0), "less than a day");
        assert_eq!(duration_phrase(1), "1 day");
        assert_eq!(duration_phrase(4), "4 days");
        assert_eq!(duration_phrase(90), "3 months");
        assert_eq!(duration_phrase(400), "1 year, 1 month");
        assert_eq!(duration_phrase(800), "2 years, 2 months");
        assert_eq!(duration_phrase(730), "2 years");
    }
}
