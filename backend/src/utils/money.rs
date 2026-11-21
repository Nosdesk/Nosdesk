//! Money: amounts in minor units (`i64`) with an ISO 4217 code.
//!
//! The one place that knows each currency's minor-unit exponent (AUD 2,
//! JPY 0, KWD 3), reads an amount someone typed or a spreadsheet holds
//! ([`parse_amount`]), and writes minor units back as a plain decimal
//! ([`format_minor`]). Display formatting belongs to the client, which has
//! the reader's locale. Amounts are never converted between currencies.

use serde::{Deserialize, Serialize};

/// A currency as ISO 4217 lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Currency {
    pub code: &'static str,
    /// Digits after the decimal mark: minor units per major unit is
    /// `10^exponent`.
    pub exponent: u8,
    /// Withdrawn codes still read and format, so a record of past work can
    /// carry the currency it was paid in, but a workspace can't pick one.
    pub withdrawn: bool,
}

const fn c(code: &'static str, exponent: u8) -> Currency {
    Currency {
        code,
        exponent,
        withdrawn: false,
    }
}

const fn w(code: &'static str, exponent: u8) -> Currency {
    Currency {
        code,
        exponent,
        withdrawn: true,
    }
}

/// ISO 4217 currencies, sorted by code: the current list (SIX list one,
/// published 2026-09-17) without fund, precious metal, bond unit and
/// testing codes, plus codes withdrawn since December 2014 (list three,
/// published 2026-01-01), each marked withdrawn. Exponents are list one's
/// minor units; `Intl` disagrees for some (IDR, HUF, IQD) and is not the
/// source.
static CURRENCIES: &[Currency] = &[
    c("AED", 2),
    c("AFN", 2),
    c("ALL", 2),
    c("AMD", 2),
    w("ANG", 2),
    c("AOA", 2),
    c("ARS", 2),
    c("AUD", 2),
    c("AWG", 2),
    c("AZN", 2),
    c("BAM", 2),
    c("BBD", 2),
    c("BDT", 2),
    w("BGN", 2),
    c("BHD", 3),
    c("BIF", 0),
    c("BMD", 2),
    c("BND", 2),
    c("BOB", 2),
    c("BRL", 2),
    c("BSD", 2),
    c("BTN", 2),
    c("BWP", 2),
    c("BYN", 2),
    w("BYR", 0),
    c("BZD", 2),
    c("CAD", 2),
    c("CDF", 2),
    c("CHF", 2),
    c("CLP", 0),
    c("CNY", 2),
    c("COP", 2),
    c("CRC", 2),
    w("CUC", 2),
    c("CUP", 2),
    c("CVE", 2),
    c("CZK", 2),
    c("DJF", 0),
    c("DKK", 2),
    c("DOP", 2),
    c("DZD", 2),
    c("EGP", 2),
    c("ERN", 2),
    c("ETB", 2),
    c("EUR", 2),
    c("FJD", 2),
    c("FKP", 2),
    c("GBP", 2),
    c("GEL", 2),
    c("GHS", 2),
    c("GIP", 2),
    c("GMD", 2),
    c("GNF", 0),
    c("GTQ", 2),
    c("GYD", 2),
    c("HKD", 2),
    c("HNL", 2),
    w("HRK", 2),
    c("HTG", 2),
    c("HUF", 2),
    c("IDR", 2),
    c("ILS", 2),
    c("INR", 2),
    c("IQD", 3),
    c("IRR", 2),
    c("ISK", 0),
    c("JMD", 2),
    c("JOD", 3),
    c("JPY", 0),
    c("KES", 2),
    c("KGS", 2),
    c("KHR", 2),
    c("KMF", 0),
    c("KPW", 2),
    c("KRW", 0),
    c("KWD", 3),
    c("KYD", 2),
    c("KZT", 2),
    c("LAK", 2),
    c("LBP", 2),
    c("LKR", 2),
    c("LRD", 2),
    c("LSL", 2),
    w("LTL", 2),
    c("LYD", 3),
    c("MAD", 2),
    c("MDL", 2),
    c("MGA", 2),
    c("MKD", 2),
    c("MMK", 2),
    c("MNT", 2),
    c("MOP", 2),
    w("MRO", 2),
    c("MRU", 2),
    c("MUR", 2),
    c("MVR", 2),
    c("MWK", 2),
    c("MXN", 2),
    c("MYR", 2),
    c("MZN", 2),
    c("NAD", 2),
    c("NGN", 2),
    c("NIO", 2),
    c("NOK", 2),
    c("NPR", 2),
    c("NZD", 2),
    c("OMR", 3),
    c("PAB", 2),
    c("PEN", 2),
    c("PGK", 2),
    c("PHP", 2),
    c("PKR", 2),
    c("PLN", 2),
    c("PYG", 0),
    c("QAR", 2),
    c("RON", 2),
    c("RSD", 2),
    c("RUB", 2),
    c("RWF", 0),
    c("SAR", 2),
    c("SBD", 2),
    c("SCR", 2),
    c("SDG", 2),
    c("SEK", 2),
    c("SGD", 2),
    c("SHP", 2),
    c("SLE", 2),
    w("SLL", 2),
    c("SOS", 2),
    c("SRD", 2),
    c("SSP", 2),
    w("STD", 2),
    c("STN", 2),
    c("SVC", 2),
    c("SYP", 2),
    c("SZL", 2),
    c("THB", 2),
    c("TJS", 2),
    c("TMT", 2),
    c("TND", 3),
    c("TOP", 2),
    c("TRY", 2),
    c("TTD", 2),
    c("TWD", 2),
    c("TZS", 2),
    c("UAH", 2),
    c("UGX", 0),
    c("USD", 2),
    c("UYU", 2),
    c("UZS", 2),
    c("VED", 2),
    w("VEF", 2),
    c("VES", 2),
    c("VND", 0),
    c("VUV", 0),
    c("WST", 2),
    c("XAF", 0),
    c("XCD", 2),
    c("XCG", 2),
    c("XOF", 0),
    c("XPF", 0),
    c("YER", 2),
    c("ZAR", 2),
    c("ZMW", 2),
    c("ZWG", 2),
    w("ZWL", 2),
];

/// The currency with ISO code `code` (upper case, as stored), withdrawn
/// ones included.
pub fn currency(code: &str) -> Option<&'static Currency> {
    CURRENCIES
        .binary_search_by(|c| c.code.cmp(code))
        .ok()
        .map(|i| &CURRENCIES[i])
}

/// Currencies a workspace can pick, by code.
pub fn current_currencies() -> impl Iterator<Item = &'static Currency> {
    CURRENCIES.iter().filter(|c| !c.withdrawn)
}

/// The currency suggested for a workspace that hasn't set one, from the
/// region of its default locale (`en-AU` AUD, `nl-NL` EUR).
pub fn suggested_currency(locale: &str) -> Option<&'static str> {
    let region = locale.rsplit(['-', '_']).next()?;
    let code = match region.to_ascii_uppercase().as_str() {
        "AU" => "AUD",
        "GB" => "GBP",
        "US" => "USD",
        "CA" => "CAD",
        "NZ" => "NZD",
        "IE" | "FR" | "NL" | "BE" | "DE" | "AT" | "ES" | "IT" | "PT" | "FI" | "LU" => "EUR",
        _ => return None,
    };
    Some(code)
}

/// The decimal mark of a typed or imported number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecimalMark {
    #[default]
    #[serde(rename = ".")]
    Dot,
    #[serde(rename = ",")]
    Comma,
}

/// Currency symbols passed over around a number.
const CURRENCY_SYMBOLS: &[char] = &['$', '€', '£', '¥', '₹', '₩', '₽', '₪', '₫', '¢'];

/// Spaces that group thousands: plain, no-break, narrow no-break, thin.
const GROUP_SPACES: &[char] = &[' ', '\u{a0}', '\u{202f}', '\u{2009}'];

/// A number as [`read_number`] reads it: sign and digits, separators gone.
#[derive(Debug, PartialEq, Eq)]
pub struct NumberText<'a> {
    pub negative: bool,
    /// Digits before the mark; empty for `.5`.
    pub whole: String,
    /// Digits after the mark; empty without one.
    pub fraction: &'a str,
    /// A three-letter code passed over before or after the number
    /// (`USD 12`, `12 EUR`).
    pub code: Option<&'a str>,
}

impl NumberText<'_> {
    /// Plain decimal text: `-1234.56`, `0` for an empty whole.
    pub fn plain(&self) -> String {
        let sign = if self.negative { "-" } else { "" };
        let whole = if self.whole.is_empty() {
            "0"
        } else {
            &self.whole
        };
        if self.fraction.is_empty() {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{whole}.{}", self.fraction)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum NumberError {
    Invalid,
    /// `1,234` under a `,` mark: one and a bit, or a thousand and more.
    Ambiguous {
        whole: String,
        fraction: String,
    },
}

/// `raw` without a leading or trailing currency symbol or code, its sign
/// (`-$7.50` and `$-7.50` are both negative), and the three-letter code
/// passed over, if any.
fn strip_currency(raw: &str) -> (bool, &str, Option<&str>) {
    fn sign(text: &str) -> (bool, &str) {
        match text.strip_prefix('-') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, text.strip_prefix('+').unwrap_or(text).trim_start()),
        }
    }
    let (mut negative, mut text) = sign(raw.trim());
    let mut code = None;
    // A code of up to three capitals before a symbol (`A$`, `US$`), a
    // symbol alone, or a three-letter code (`USD 12`, `EUR12`).
    let letters = text.chars().take_while(char::is_ascii_uppercase).count();
    let after = &text[letters..];
    if letters <= 3 && after.starts_with(CURRENCY_SYMBOLS) {
        let symbol = after.chars().next().map_or(0, char::len_utf8);
        text = after[symbol..].trim_start();
    } else if letters == 3 {
        code = Some(&text[..3]);
        text = after.trim_start();
    }
    if !negative {
        (negative, text) = sign(text);
    }
    if let Some(rest) = text.strip_suffix(CURRENCY_SYMBOLS) {
        text = rest.trim_end();
    } else {
        let letters = text
            .chars()
            .rev()
            .take_while(char::is_ascii_uppercase)
            .count();
        if letters == 3 {
            code = Some(&text[text.len() - 3..]);
            text = text[..text.len() - 3].trim_end();
        }
    }
    (negative, text, code)
}

/// Read a number as someone typed it or a spreadsheet holds it.
///
/// A currency symbol or code before or after the number is passed over
/// (`$7,450.00`, `A$12`, `7 450 €`, `EUR 12`). The thousands separator is
/// the other mark (`,` for a `.` mark, `.` for a `,` mark) or a space, and
/// is taken only in groups of three (`1,234.56`, `1.234,56`, `1 234,56`).
/// Under a `,` mark, one `,` followed by three digits (`1,234`) could be
/// either and is refused rather than guessed.
pub fn read_number(raw: &str, mark: DecimalMark) -> Result<NumberText<'_>, NumberError> {
    let (negative, body, code) = strip_currency(raw);
    let (decimal, group) = match mark {
        DecimalMark::Dot => ('.', ','),
        DecimalMark::Comma => (',', '.'),
    };
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let mut parts = body.split(decimal);
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    if parts.next().is_some() || !fraction.is_none_or(digits) {
        return Err(NumberError::Invalid);
    }
    let grouped = whole.contains(group) || whole.contains(GROUP_SPACES);
    let whole = if grouped {
        // One separator throughout, a lead group of one to three digits,
        // then groups of three.
        let separator = whole.chars().find(|c| !c.is_ascii_digit()).unwrap_or(group);
        if separator != group && !GROUP_SPACES.contains(&separator) {
            return Err(NumberError::Invalid);
        }
        let groups: Vec<&str> = whole.split(separator).collect();
        let lead = groups[0];
        let lead_ok = (1..=3).contains(&lead.len()) && digits(lead);
        if !lead_ok || groups[1..].iter().any(|g| g.len() != 3 || !digits(g)) {
            return Err(NumberError::Invalid);
        }
        groups.concat()
    } else {
        if !digits(whole) || (whole.is_empty() && fraction.is_none_or(str::is_empty)) {
            return Err(NumberError::Invalid);
        }
        whole.to_string()
    };
    let fraction = fraction.unwrap_or("");
    if mark == DecimalMark::Comma
        && !grouped
        && fraction.len() == 3
        && (1..=3).contains(&whole.len())
        && !whole.starts_with('0')
    {
        return Err(NumberError::Ambiguous {
            whole,
            fraction: fraction.to_string(),
        });
    }
    Ok(NumberText {
        negative,
        whole,
        fraction,
        code,
    })
}

/// Why an amount doesn't read.
#[derive(Debug, PartialEq, Eq)]
pub enum MoneyError {
    UnknownCurrency(String),
    /// Not a number under the decimal mark.
    Invalid,
    /// `1,234` under a `,` mark.
    Ambiguous,
    /// A code written with the amount that isn't the amount's currency.
    CurrencyMismatch {
        written: String,
        currency: String,
    },
    /// More non-zero decimals than the currency has minor units.
    TooManyDecimals {
        exponent: u8,
    },
    /// Beyond what `i64` minor units hold.
    TooLarge,
}

impl std::fmt::Display for MoneyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCurrency(code) => write!(f, "'{code}' is not an ISO 4217 currency code"),
            Self::Invalid => f.write_str("not a valid amount"),
            Self::Ambiguous => f.write_str(
                "the amount could be read two ways; write it with a thousands separator or more decimals",
            ),
            Self::CurrencyMismatch { written, currency } => {
                write!(f, "the amount is written in {written}, not {currency}")
            }
            Self::TooManyDecimals { exponent: 0 } => f.write_str("takes no decimals"),
            Self::TooManyDecimals { exponent: 1 } => f.write_str("takes at most 1 decimal"),
            Self::TooManyDecimals { exponent } => write!(f, "takes at most {exponent} decimals"),
            Self::TooLarge => f.write_str("the amount is too large"),
        }
    }
}

impl std::error::Error for MoneyError {}

/// `text` in minor units of `code`: `1,234.56` AUD under a `.` mark is
/// `123456`. Negative amounts read as negative; callers that take none
/// refuse them. Decimals beyond the currency's exponent are refused unless
/// they are zeros (`12.500` USD is `1250`), so nothing is ever rounded.
pub fn parse_amount(text: &str, code: &str, mark: DecimalMark) -> Result<i64, MoneyError> {
    let currency = currency(code).ok_or_else(|| MoneyError::UnknownCurrency(code.to_string()))?;
    let number = read_number(text.trim(), mark).map_err(|e| match e {
        NumberError::Invalid => MoneyError::Invalid,
        NumberError::Ambiguous { .. } => MoneyError::Ambiguous,
    })?;
    if let Some(written) = number.code.filter(|w| *w != currency.code) {
        return Err(MoneyError::CurrencyMismatch {
            written: written.to_string(),
            currency: currency.code.to_string(),
        });
    }
    let exponent = usize::from(currency.exponent);
    let (kept, extra) = number
        .fraction
        .split_at(number.fraction.len().min(exponent));
    if extra.bytes().any(|b| b != b'0') {
        return Err(MoneyError::TooManyDecimals {
            exponent: currency.exponent,
        });
    }
    // Every digit after the whole part, padded to the exponent: `12.5` AUD
    // reads as the digits `1250`.
    let digits = number
        .whole
        .bytes()
        .chain(kept.bytes())
        .chain(std::iter::repeat_n(b'0', exponent - kept.len()));
    let mut minor: i64 = 0;
    for d in digits {
        let d = i64::from(d - b'0');
        minor = minor
            .checked_mul(10)
            .and_then(|m| {
                if number.negative {
                    m.checked_sub(d)
                } else {
                    m.checked_add(d)
                }
            })
            .ok_or(MoneyError::TooLarge)?;
    }
    Ok(minor)
}

/// `minor` units of `code` as a plain decimal with the currency's
/// exponent: `125050` AUD is `1250.50`, `1250` JPY is `1250`, `-5` KWD is
/// `-0.005`.
pub fn format_minor(minor: i64, code: &str) -> Result<String, MoneyError> {
    let currency = currency(code).ok_or_else(|| MoneyError::UnknownCurrency(code.to_string()))?;
    let sign = if minor < 0 { "-" } else { "" };
    let digits = minor.unsigned_abs().to_string();
    let exponent = usize::from(currency.exponent);
    if exponent == 0 {
        return Ok(format!("{sign}{digits}"));
    }
    let digits = format!("{digits:0>width$}", width = exponent + 1);
    let (whole, fraction) = digits.split_at(digits.len() - exponent);
    Ok(format!("{sign}{whole}.{fraction}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOT: DecimalMark = DecimalMark::Dot;
    const COMMA: DecimalMark = DecimalMark::Comma;

    #[test]
    fn the_table_is_sorted_upper_case_and_unique() {
        for pair in CURRENCIES.windows(2) {
            assert!(
                pair[0].code < pair[1].code,
                "{} then {}",
                pair[0].code,
                pair[1].code
            );
        }
        for c in CURRENCIES {
            assert!(
                c.code.len() == 3 && c.code.bytes().all(|b| b.is_ascii_uppercase()),
                "{}",
                c.code
            );
            assert!(c.exponent <= 3, "{}", c.code);
        }
    }

    #[test]
    fn currencies_know_their_exponent() {
        assert_eq!(currency("AUD").map(|c| c.exponent), Some(2));
        assert_eq!(currency("JPY").map(|c| c.exponent), Some(0));
        assert_eq!(currency("KWD").map(|c| c.exponent), Some(3));
        assert_eq!(currency("aud"), None);
        assert_eq!(currency("ZZZ"), None);
        // Withdrawn codes read, but aren't offered.
        assert!(currency("HRK").is_some_and(|c| c.withdrawn));
        assert!(current_currencies().all(|c| c.code != "HRK"));
        assert!(current_currencies().any(|c| c.code == "EUR"));
    }

    #[test]
    fn locales_suggest_their_region_s_currency() {
        assert_eq!(suggested_currency("en-AU"), Some("AUD"));
        assert_eq!(suggested_currency("en-GB"), Some("GBP"));
        assert_eq!(suggested_currency("en-US"), Some("USD"));
        assert_eq!(suggested_currency("fr-FR"), Some("EUR"));
        assert_eq!(suggested_currency("nl-NL"), Some("EUR"));
        assert_eq!(suggested_currency("en"), None);
        assert_eq!(suggested_currency(""), None);
        for code in ["AUD", "GBP", "USD", "CAD", "NZD", "EUR"] {
            assert!(currency(code).is_some_and(|c| !c.withdrawn), "{code}");
        }
    }

    #[test]
    fn amounts_read_in_minor_units_for_each_exponent() {
        assert_eq!(parse_amount("1250.50", "AUD", DOT), Ok(125050));
        assert_eq!(parse_amount("1250.5", "AUD", DOT), Ok(125050));
        assert_eq!(parse_amount("1250", "AUD", DOT), Ok(125000));
        assert_eq!(parse_amount(".5", "AUD", DOT), Ok(50));
        assert_eq!(parse_amount("0", "AUD", DOT), Ok(0));
        assert_eq!(parse_amount("1250", "JPY", DOT), Ok(1250));
        assert_eq!(parse_amount("1.250", "KWD", DOT), Ok(1250));
        assert_eq!(parse_amount("1.25", "KWD", DOT), Ok(1250));
        assert_eq!(parse_amount(" 12.34 ", "USD", DOT), Ok(1234));
    }

    #[test]
    fn both_marks_read_with_their_thousands_separators() {
        assert_eq!(parse_amount("1,234.56", "AUD", DOT), Ok(123456));
        assert_eq!(parse_amount("1.234,56", "EUR", COMMA), Ok(123456));
        assert_eq!(parse_amount("1 234,56", "EUR", COMMA), Ok(123456));
        assert_eq!(parse_amount("1\u{202f}234,56", "EUR", COMMA), Ok(123456));
        assert_eq!(parse_amount("12,5", "EUR", COMMA), Ok(1250));
        assert_eq!(parse_amount("1,234", "AUD", DOT), Ok(123400));
        assert_eq!(parse_amount("1.234.567", "JPY", COMMA), Ok(1234567));
        // The other mark's convention doesn't read.
        assert_eq!(
            parse_amount("1.234,56", "EUR", DOT),
            Err(MoneyError::Invalid)
        );
        assert_eq!(parse_amount("12,5", "AUD", DOT), Err(MoneyError::Invalid));
        assert_eq!(parse_amount("12.5", "EUR", COMMA), Err(MoneyError::Invalid));
        // One `,` and three digits under a `,` mark could be either.
        assert_eq!(
            parse_amount("1,234", "EUR", COMMA),
            Err(MoneyError::Ambiguous)
        );
        assert_eq!(
            parse_amount("1,23,456", "INR", DOT),
            Err(MoneyError::Invalid)
        );
    }

    #[test]
    fn symbols_and_the_amount_s_own_code_are_passed_over() {
        assert_eq!(parse_amount("$7,450.00", "AUD", DOT), Ok(745000));
        assert_eq!(parse_amount("€ 1.234,56", "EUR", COMMA), Ok(123456));
        assert_eq!(parse_amount("USD 99.95", "USD", DOT), Ok(9995));
        assert_eq!(parse_amount("99,95 EUR", "EUR", COMMA), Ok(9995));
        assert_eq!(
            parse_amount("USD 99.95", "AUD", DOT),
            Err(MoneyError::CurrencyMismatch {
                written: "USD".into(),
                currency: "AUD".into()
            })
        );
    }

    #[test]
    fn decimals_beyond_the_exponent_are_refused_unless_zero() {
        assert_eq!(
            parse_amount("12.345", "USD", DOT),
            Err(MoneyError::TooManyDecimals { exponent: 2 })
        );
        assert_eq!(
            parse_amount("1250.5", "JPY", DOT),
            Err(MoneyError::TooManyDecimals { exponent: 0 })
        );
        assert_eq!(
            parse_amount("1.2345", "KWD", DOT),
            Err(MoneyError::TooManyDecimals { exponent: 3 })
        );
        assert_eq!(parse_amount("12.500", "USD", DOT), Ok(1250));
        assert_eq!(parse_amount("1250.00", "JPY", DOT), Ok(1250));
    }

    #[test]
    fn negatives_read_and_callers_refuse_them() {
        assert_eq!(parse_amount("-12.50", "USD", DOT), Ok(-1250));
        assert_eq!(parse_amount("-$1,234.5", "USD", DOT), Ok(-123450));
    }

    #[test]
    fn amounts_beyond_i64_are_refused() {
        assert_eq!(
            parse_amount("92233720368547758.07", "USD", DOT),
            Ok(i64::MAX)
        );
        assert_eq!(
            parse_amount("92233720368547758.08", "USD", DOT),
            Err(MoneyError::TooLarge)
        );
        assert_eq!(
            parse_amount("-92233720368547758.08", "USD", DOT),
            Ok(i64::MIN)
        );
        assert_eq!(
            parse_amount("-92233720368547758.09", "USD", DOT),
            Err(MoneyError::TooLarge)
        );
        assert_eq!(
            parse_amount("99999999999999999999999", "JPY", DOT),
            Err(MoneyError::TooLarge)
        );
    }

    #[test]
    fn junk_and_unknown_codes_are_refused() {
        assert_eq!(parse_amount("", "USD", DOT), Err(MoneyError::Invalid));
        assert_eq!(parse_amount("$", "USD", DOT), Err(MoneyError::Invalid));
        assert_eq!(parse_amount("two", "USD", DOT), Err(MoneyError::Invalid));
        assert_eq!(
            parse_amount("12 apples", "USD", DOT),
            Err(MoneyError::Invalid)
        );
        assert_eq!(parse_amount("1e5", "USD", DOT), Err(MoneyError::Invalid));
        assert_eq!(
            parse_amount("12", "ZZZ", DOT),
            Err(MoneyError::UnknownCurrency("ZZZ".into()))
        );
    }

    #[test]
    fn minor_units_format_with_the_currency_s_exponent() {
        assert_eq!(format_minor(125050, "AUD").as_deref(), Ok("1250.50"));
        assert_eq!(format_minor(5, "AUD").as_deref(), Ok("0.05"));
        assert_eq!(format_minor(0, "AUD").as_deref(), Ok("0.00"));
        assert_eq!(format_minor(-1250, "USD").as_deref(), Ok("-12.50"));
        assert_eq!(format_minor(1250, "JPY").as_deref(), Ok("1250"));
        assert_eq!(format_minor(-5, "KWD").as_deref(), Ok("-0.005"));
        assert_eq!(format_minor(1250, "KWD").as_deref(), Ok("1.250"));
        assert_eq!(
            format_minor(i64::MIN, "USD").as_deref(),
            Ok("-92233720368547758.08")
        );
        assert!(format_minor(1, "ZZZ").is_err());
        // What formats reads back.
        for (minor, code) in [(125050, "AUD"), (7, "KWD"), (-99, "JPY"), (i64::MAX, "USD")] {
            let text = format_minor(minor, code).unwrap();
            assert_eq!(parse_amount(&text, code, DOT), Ok(minor), "{text} {code}");
        }
    }
}
