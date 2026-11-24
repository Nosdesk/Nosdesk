//! Display order for a field schema's properties. jsonb keeps no key order,
//! so kinds, record types and the user field schema store the order beside
//! the schema in `field_order`, kept equal to the schema's key set.

use std::collections::HashSet;

use serde_json::Value;

/// `order` as a permutation of the schema's property names: unknown and
/// repeated names dropped, missing ones appended in key order (the order a
/// schema with no stored order has always been shown in).
pub fn normalise_order(schema: &Value, order: &[String]) -> Vec<String> {
    let keys: Vec<String> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut out: Vec<String> = order
        .iter()
        .filter(|k| keys.contains(k) && seen.insert(k.as_str().to_string()))
        .cloned()
        .collect();
    for key in keys {
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// A stored `text[]` order (diesel reads its elements as nullable).
pub fn stored_order(order: &[Option<String>]) -> Vec<String> {
    order.iter().flatten().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(keys: &[&str]) -> Value {
        let properties: serde_json::Map<String, Value> = keys
            .iter()
            .map(|k| (k.to_string(), json!({ "type": "string" })))
            .collect();
        json!({ "type": "object", "properties": properties })
    }

    fn order(keys: &[&str]) -> Vec<String> {
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn keeps_the_given_order_of_known_keys() {
        let s = schema(&["a", "b", "c"]);
        assert_eq!(
            normalise_order(&s, &order(&["c", "a", "b"])),
            ["c", "a", "b"]
        );
    }

    #[test]
    fn drops_unknown_and_repeated_names_and_appends_missing_ones_in_key_order() {
        let s = schema(&["b", "aa", "Zed", "c"]);
        assert_eq!(
            normalise_order(&s, &order(&["c", "gone", "c", "b"])),
            ["c", "b", "Zed", "aa"]
        );
    }

    #[test]
    fn an_empty_order_is_key_byte_order() {
        let s = schema(&["b", "aa", "Zed"]);
        assert_eq!(normalise_order(&s, &[]), ["Zed", "aa", "b"]);
        assert!(normalise_order(&json!({ "type": "object" }), &order(&["a"])).is_empty());
    }
}
