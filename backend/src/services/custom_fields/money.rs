//! Money values in custom fields: `{ "amount": "1250.50", "currency": "AUD" }`,
//! the shape the records API takes a cost in. The amount is a decimal string
//! canonical to its currency's exponent (`utils::money`), so equal amounts
//! are equal text and sums are exact.
//!
//! A write may leave the currency out, or give the amount as a number or
//! with extra zeros; [`normalise_values`] fills and canonicalises it before
//! the validator, which then needs no database read and accepts only the
//! canonical shape.

use serde_json::{json, Map, Value};

use super::schema::AttributeError;
use crate::utils::money::{self, DecimalMark};

/// Whether a property holds money.
pub fn is_money(prop: &Value) -> bool {
    prop.get("type").and_then(Value::as_str) == Some("object")
        && prop.get("format").and_then(Value::as_str) == Some("money")
}

/// The currency a property pins its values to, if any.
pub fn pinned_currency(prop: &Value) -> Option<&str> {
    prop.get("currency").and_then(Value::as_str)
}

/// `values` with each money value made canonical: its currency filled from
/// the property's pinned code, else `default_currency` (the workspace's),
/// and its amount written to the currency's exponent. Values that aren't
/// money, and money values that aren't objects, are left for the validator.
pub fn normalise_values(
    schema: &Value,
    values: &Value,
    default_currency: Option<&str>,
) -> Result<Value, AttributeError> {
    let (Some(props), Some(map)) = (
        schema.get("properties").and_then(Value::as_object),
        values.as_object(),
    ) else {
        return Ok(values.clone());
    };
    let mut out = map.clone();
    for (key, value) in out.iter_mut() {
        let Some(prop) = props.get(key).filter(|p| is_money(p)) else {
            continue;
        };
        if let Value::Object(money) = value {
            *value = normalise_money(key, prop, money, default_currency)?;
        }
    }
    Ok(Value::Object(out))
}

fn normalise_money(
    property: &str,
    prop: &Value,
    value: &Map<String, Value>,
    default_currency: Option<&str>,
) -> Result<Value, AttributeError> {
    let bad = |reason: String| AttributeError::BadMoney {
        property: property.to_string(),
        reason,
    };
    let given = match value.get("currency") {
        None | Some(Value::Null) => None,
        Some(Value::String(code)) if !code.trim().is_empty() => Some(code.trim().to_uppercase()),
        Some(_) => return Err(bad("currency must be an ISO 4217 code".into())),
    };
    let pinned = pinned_currency(prop);
    if let (Some(given), Some(pinned)) = (&given, pinned) {
        if given != pinned {
            return Err(AttributeError::CurrencyNotAllowed {
                property: property.to_string(),
                currency: pinned.to_string(),
            });
        }
    }
    let code = given
        .or_else(|| pinned.map(str::to_string))
        .or_else(|| default_currency.map(str::to_string))
        .ok_or_else(|| bad("needs a currency while the workspace has none".into()))?;
    let text = match value.get("amount") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => {
            return Err(bad(
                "amount must be a decimal string such as \"1250.50\"".into()
            ))
        }
    };
    let minor = money::parse_amount(&text, &code, DecimalMark::Dot).map_err(|e| match e {
        money::MoneyError::UnknownCurrency(code) => {
            bad(format!("'{code}' is not an ISO 4217 currency code"))
        }
        other => bad(format!("amount {other}")),
    })?;
    let amount = money::format_minor(minor, &code).map_err(|e| bad(e.to_string()))?;
    Ok(json!({ "amount": amount, "currency": code }))
}

/// A canonical money value's amount in minor units, or why it isn't one.
pub fn check_money(prop: &Value, value: &Value) -> Result<(i64, String), String> {
    let map = value
        .as_object()
        .ok_or_else(|| "must be an object with amount and currency".to_string())?;
    if let Some(extra) = map.keys().find(|k| *k != "amount" && *k != "currency") {
        return Err(format!("has an unexpected key `{extra}`"));
    }
    let code = map
        .get("currency")
        .and_then(Value::as_str)
        .ok_or_else(|| "needs a currency".to_string())?;
    if money::currency(code).is_none() {
        return Err(format!("'{code}' is not an ISO 4217 currency code"));
    }
    if let Some(pinned) = pinned_currency(prop) {
        if pinned != code {
            return Err(format!("must be in {pinned}"));
        }
    }
    let amount = map
        .get("amount")
        .and_then(Value::as_str)
        .ok_or_else(|| "amount must be a decimal string".to_string())?;
    let minor =
        money::parse_amount(amount, code, DecimalMark::Dot).map_err(|e| format!("amount {e}"))?;
    let canonical = money::format_minor(minor, code).map_err(|e| e.to_string())?;
    if canonical != amount {
        return Err(format!("amount must be written as {canonical}"));
    }
    Ok((minor, code.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "cost": { "type": "object", "format": "money" },
                "usd": { "type": "object", "format": "money", "currency": "USD" },
                "note": { "type": "string" }
            }
        })
    }

    #[test]
    fn fills_the_currency_and_canonicalises_the_amount() {
        let out = normalise_values(
            &schema(),
            &json!({
                "cost": { "amount": 1250.5 },
                "usd": { "amount": "7" },
                "note": "kept"
            }),
            Some("AUD"),
        )
        .unwrap();
        assert_eq!(
            out,
            json!({
                "cost": { "amount": "1250.50", "currency": "AUD" },
                "usd": { "amount": "7.00", "currency": "USD" },
                "note": "kept"
            })
        );

        let jpy = normalise_values(
            &schema(),
            &json!({ "cost": { "amount": "1250.000", "currency": "jpy" } }),
            Some("AUD"),
        )
        .unwrap();
        assert_eq!(jpy["cost"], json!({ "amount": "1250", "currency": "JPY" }));
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let refused = |value: Value, default: Option<&str>| {
            normalise_values(&schema(), &json!({ "cost": value }), default).unwrap_err()
        };
        assert!(matches!(
            refused(json!({ "amount": "1" }), None),
            AttributeError::BadMoney { .. }
        ));
        assert!(matches!(
            refused(json!({ "amount": "1.234", "currency": "AUD" }), None),
            AttributeError::BadMoney { .. }
        ));
        assert!(matches!(
            refused(json!({ "amount": "1", "currency": "ZZZ" }), None),
            AttributeError::BadMoney { .. }
        ));
        assert!(matches!(
            normalise_values(
                &schema(),
                &json!({ "usd": { "amount": "1", "currency": "AUD" } }),
                None
            )
            .unwrap_err(),
            AttributeError::CurrencyNotAllowed { .. }
        ));
    }

    #[test]
    fn checks_the_canonical_shape() {
        let prop = json!({ "type": "object", "format": "money" });
        assert_eq!(
            check_money(&prop, &json!({ "amount": "12.50", "currency": "AUD" })),
            Ok((1250, "AUD".to_string()))
        );
        assert!(check_money(&prop, &json!({ "amount": "12.5", "currency": "AUD" })).is_err());
        assert!(check_money(&prop, &json!({ "amount": "12", "currency": "KWD" })).is_err());
        assert!(check_money(&prop, &json!({ "amount": "12.000", "currency": "KWD" })).is_ok());
        assert!(check_money(&prop, &json!({ "amount": "1", "currency": "ZZZ" })).is_err());
        assert!(check_money(&prop, &json!({ "amount": "1.00" })).is_err());
        assert!(check_money(&prop, &json!({ "amount": 1, "currency": "AUD" })).is_err());
        assert!(check_money(
            &prop,
            &json!({ "amount": "1.00", "currency": "AUD", "amount_minor": 100 })
        )
        .is_err());
        let pinned = json!({ "type": "object", "format": "money", "currency": "USD" });
        assert!(check_money(&pinned, &json!({ "amount": "1.00", "currency": "AUD" })).is_err());
    }
}
