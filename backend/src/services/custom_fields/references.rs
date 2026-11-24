//! Reference fields: values that name a person (`user-uuid`), an asset
//! (`asset-ref`, scoped by `assetKind`) or a people group (`group-ref`),
//! alone or as array items.
//!
//! A write's references are checked against the workspace only where the
//! write adds or changes them: a value already stored is kept as it is, so a
//! record whose old reference points at something since deleted still saves,
//! and the stale value renders as removed.

use std::collections::{HashMap, HashSet};

use diesel::QueryResult;
use serde_json::Value;
use uuid::Uuid;

use super::schema::AttributeError;
use crate::db::DbConnection;
use crate::repository::{assets as asset_repo, groups as group_repo, workspaces};

/// What a reference property names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    User,
    /// An asset, of this kind when the property scopes one.
    Asset(Option<String>),
    Group,
}

/// What a property's values (or its array's items) name, if anything.
pub fn target_of(prop: &Value) -> Option<Target> {
    let scalar = |p: &Value| match p.get("format").and_then(Value::as_str) {
        Some("user-uuid") => Some(Target::User),
        Some("asset-ref") => Some(Target::Asset(
            prop.get("assetKind")
                .or_else(|| p.get("assetKind"))
                .and_then(Value::as_str)
                .map(str::to_string),
        )),
        Some("group-ref") => Some(Target::Group),
        _ => None,
    };
    scalar(prop).or_else(|| prop.get("items").and_then(scalar))
}

/// The reference values `values` adds over `previous`, by property: a
/// changed scalar, or array items `previous` doesn't hold.
pub fn added_references(
    schema: &Value,
    values: &Value,
    previous: Option<&Value>,
) -> Vec<(String, Target, String)> {
    let (Some(props), Some(map)) = (
        schema.get("properties").and_then(Value::as_object),
        values.as_object(),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in map {
        let Some(target) = props.get(key).and_then(target_of) else {
            continue;
        };
        let before = previous.and_then(|p| p.get(key));
        let held: HashSet<&str> = match before {
            Some(Value::String(s)) => HashSet::from([s.as_str()]),
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
            _ => HashSet::new(),
        };
        let now: Vec<&str> = match value {
            Value::String(s) => vec![s.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for v in now {
            if !held.contains(v) {
                out.push((key.clone(), target.clone(), v.to_string()));
            }
        }
    }
    out
}

/// The people, assets and people groups a check reads references against.
#[derive(Debug, Default, Clone)]
pub struct Known {
    /// Active members of the workspace.
    pub members: HashSet<Uuid>,
    /// Each asset's kind.
    pub asset_kinds: HashMap<i32, String>,
    /// People groups.
    pub groups: HashSet<i32>,
}

/// Refuse a reference `values` adds over `previous` that names no member
/// of the workspace, no asset (or one of another kind than the property
/// scopes), or no people group of `known`. Values are assumed well formed:
/// the validator has run.
pub fn check_known(
    schema: &Value,
    values: &Value,
    previous: Option<&Value>,
    known: &Known,
) -> Result<(), AttributeError> {
    for (property, target, value) in added_references(schema, values, previous) {
        let missing = || AttributeError::MissingReference {
            property: property.clone(),
            value: value.clone(),
        };
        match target {
            Target::User => {
                if !Uuid::parse_str(&value).is_ok_and(|u| known.members.contains(&u)) {
                    return Err(missing());
                }
            }
            Target::Asset(scope) => {
                let Some(kind) = value
                    .parse::<i32>()
                    .ok()
                    .and_then(|id| known.asset_kinds.get(&id))
                else {
                    return Err(missing());
                };
                if let Some(scope) = scope.filter(|s| s != kind) {
                    return Err(AttributeError::WrongAssetKind {
                        property,
                        value,
                        kind: scope,
                    });
                }
            }
            Target::Group => {
                if !value
                    .parse::<i32>()
                    .is_ok_and(|id| known.groups.contains(&id))
                {
                    return Err(missing());
                }
            }
        }
    }
    Ok(())
}

/// [`check_known`] against the pinned workspace, reading only the records
/// the write adds.
pub fn check_references(
    conn: &mut DbConnection,
    schema: &Value,
    values: &Value,
    previous: Option<&Value>,
) -> QueryResult<Result<(), AttributeError>> {
    let added = added_references(schema, values, previous);
    if added.is_empty() {
        return Ok(Ok(()));
    }
    let users: Vec<Uuid> = added
        .iter()
        .filter(|(_, t, _)| *t == Target::User)
        .filter_map(|(_, _, v)| Uuid::parse_str(v).ok())
        .collect();
    let asset_ids: Vec<i32> = added
        .iter()
        .filter(|(_, t, _)| matches!(t, Target::Asset(_)))
        .filter_map(|(_, _, v)| v.parse().ok())
        .collect();
    let group_ids: Vec<i32> = added
        .iter()
        .filter(|(_, t, _)| *t == Target::Group)
        .filter_map(|(_, _, v)| v.parse().ok())
        .collect();
    let known = Known {
        members: if users.is_empty() {
            HashSet::new()
        } else {
            workspaces::active_members_among(conn, &users)?
        },
        asset_kinds: if asset_ids.is_empty() {
            HashMap::new()
        } else {
            asset_repo::kinds_of(conn, &asset_ids)?
        },
        groups: if group_ids.is_empty() {
            HashSet::new()
        } else {
            group_repo::existing_ids(conn, &group_ids)?
        },
    };
    Ok(check_known(schema, values, previous, &known))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_added_values_are_checked() {
        let schema = json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "format": "user-uuid" },
                "dock": { "type": "string", "format": "asset-ref", "assetKind": "dock" },
                "spares": { "type": "array", "items": { "type": "string", "format": "asset-ref" } },
                "team": { "type": "string", "format": "group-ref" },
                "bay": { "type": "string" }
            }
        });
        let before = json!({ "owner": "u1", "dock": "4", "spares": ["7", "8"], "bay": "x" });
        let after =
            json!({ "owner": "u1", "dock": "5", "spares": ["8", "9"], "team": "3", "bay": "y" });
        let mut added = added_references(&schema, &after, Some(&before));
        added.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            added,
            vec![
                (
                    "dock".to_string(),
                    Target::Asset(Some("dock".to_string())),
                    "5".to_string()
                ),
                ("spares".to_string(), Target::Asset(None), "9".to_string()),
                ("team".to_string(), Target::Group, "3".to_string()),
            ]
        );
        assert!(added_references(&schema, &before, Some(&before)).is_empty());
        assert_eq!(added_references(&schema, &before, None).len(), 4);
    }
}
