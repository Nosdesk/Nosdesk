//! Serde helper for nullable fields in a partial update, where an absent key
//! and an explicit `null` mean different things.
//!
//! Use with `#[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]`
//! on an `Option<Option<T>>` field: an absent key stays `None` (leave the
//! column alone), `null` becomes `Some(None)` (clear it), and a value becomes
//! `Some(Some(v))`. Without it serde reads `null` as `None`, so a client can't
//! clear the column.

use serde::{Deserialize, Deserializer};

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Patch {
        #[serde(default, deserialize_with = "super::deserialize")]
        field: Option<Option<String>>,
    }

    fn parse(json: &str) -> Option<Option<String>> {
        serde_json::from_str::<Patch>(json).unwrap().field
    }

    #[test]
    fn absent_null_and_value_are_distinct() {
        assert_eq!(parse("{}"), None);
        assert_eq!(parse(r#"{"field":null}"#), Some(None));
        assert_eq!(parse(r#"{"field":"x"}"#), Some(Some("x".to_string())));
    }
}
