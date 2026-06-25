//! Shared LDAP attribute extraction. Used by BOTH the login path (auth.rs) and
//! the directory sync (sync.rs): they MUST render an attribute the same way, or
//! the same directory user would key on a different `external_id` and split into
//! two identities. In particular AD's `objectGUID` is a 16-byte binary value;
//! both paths render it as the same lowercase hex here.

use ldap3::SearchEntry;
use serde_json::Value;

/// The configured LDAP attribute name for a logical field, from the workspace's
/// `attribute_map`, falling back to `default`. Empty strings count as unset.
pub fn attr_name(map: &Value, key: &str, default: &str) -> String {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// The first value of `attr` on the entry: a string value if present, otherwise
/// a binary value rendered as stable lowercase hex (AD `objectGUID`). `None`
/// when the attribute is absent.
pub fn first_value(entry: &SearchEntry, attr: &str) -> Option<String> {
    if let Some(v) = entry.attrs.get(attr).and_then(|v| v.first()) {
        return Some(v.clone());
    }
    entry
        .bin_attrs
        .get(attr)
        .and_then(|v| v.first())
        .map(|bytes| hex_lower(bytes))
}

/// All string values of `attr` (multi-valued attributes like telephoneNumber).
/// Binary-only values are skipped — the multi-valued fields we read are text.
pub fn all_values(entry: &SearchEntry, attr: &str) -> Vec<String> {
    entry.attrs.get(attr).cloned().unwrap_or_default()
}

/// The first value of `attr` as raw bytes (binary attributes like AD
/// `objectSid`, which we need unhexed to compute the primary-group SID). `None`
/// when absent or present only as a non-binary string.
pub fn first_bin_value(entry: &SearchEntry, attr: &str) -> Option<Vec<u8>> {
    entry.bin_attrs.get(attr).and_then(|v| v.first()).cloned()
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attr_name_falls_back_to_default() {
        let map = json!({ "email": "userPrincipalName" });
        assert_eq!(attr_name(&map, "email", "mail"), "userPrincipalName");
        assert_eq!(attr_name(&map, "external_id", "objectGUID"), "objectGUID");
        let map = json!({ "email": "" });
        assert_eq!(attr_name(&map, "email", "mail"), "mail");
    }

    #[test]
    fn hex_rendering_is_stable_lowercase() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xff, 0xab]), "000fffab");
    }
}
