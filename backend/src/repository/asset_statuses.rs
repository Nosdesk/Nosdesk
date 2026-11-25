//! Named asset statuses: a workspace's own names inside the nine fixed
//! categories.
//!
//! A status belongs to one category for life, and its slug never changes.
//! Its label is unique among live statuses (ignoring case), so an import
//! cell or API value names one; a category has at most one live default,
//! the name its plain assets read as. Making a status the default takes the
//! flag from the one that had it.
//!
//! No status is seeded. Assets, lifecycle events and loans hold a status by
//! id with `ON DELETE NO ACTION`, so a used status is archived, not deleted:
//! the rows keep it and pickers leave it out.
//!
//! Statuses are workspace configuration, not a sync aggregate: clients read
//! them over REST (`GET /assets/statuses`) and refetch after a save, and the
//! audit trigger records every change.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::Utc;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};
use diesel::QueryResult;
use regex::Regex;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{AssetNamedStatus, AssetNamedStatusChange, AssetStatus, NewAssetNamedStatus};
use crate::schema::{asset_kinds, asset_lifecycle_events, asset_loans, asset_statuses, assets};

static KEY: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-z][a-z0-9_]{0,63}$").expect("key pattern")
});

static COLOR: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^#[0-9A-Fa-f]{6}$").expect("color pattern")
});

/// The longest name, in characters.
pub const LABEL_MAX: usize = 64;

/// Why a named status write was refused.
#[derive(Debug)]
pub enum StatusError {
    NotFound,
    /// Not one of the nine category keys.
    UnknownCategory(String),
    /// A slug isn't lower case letters, digits and underscores, starting
    /// with a letter.
    InvalidKey(String),
    SlugTaken,
    /// The slug is a category key, which a status cell or value names as
    /// the plain category.
    SlugReserved,
    LabelRequired,
    /// Another live status already has the name.
    LabelTaken,
    /// The name is a category's (its key or its name, ignoring case,
    /// spacing, `_` and `-`), which an import cell names as the plain
    /// category.
    LabelReserved,
    TooLong {
        field: &'static str,
        max: usize,
    },
    /// A color that isn't `#rrggbb`.
    InvalidColor(String),
    UnknownKind(String),
    /// A default names plain assets of every kind, so it isn't limited to
    /// kinds.
    DefaultLimitedToKinds,
    /// Assets, lifecycle events or loans hold the status; archive it.
    InUse {
        count: i64,
    },
    /// `ensure` found the name on a status of another category, or a
    /// reorder named one.
    OtherCategory {
        category: String,
    },
    /// Another status became the category's default at the same time.
    DefaultTaken,
    Db(Error),
}

impl From<Error> for StatusError {
    fn from(e: Error) -> Self {
        match unique_violation(&e) {
            Some("asset_statuses_label_key") => StatusError::LabelTaken,
            Some("asset_statuses_workspace_slug_key") => StatusError::SlugTaken,
            Some("asset_statuses_default_key") => StatusError::DefaultTaken,
            _ => StatusError::Db(e),
        }
    }
}

/// The unique index a write violated, if that is why it failed.
fn unique_violation(e: &Error) -> Option<&str> {
    match e {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => info.constraint_name(),
        _ => None,
    }
}

/// A status to create.
#[derive(Debug, Clone)]
pub struct NewStatusInput {
    pub category: String,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub position: i32,
    pub is_default: bool,
    pub applies_to_kinds: Option<Vec<String>>,
    pub created_by: Option<Uuid>,
}

/// An edit to a status. Outer `None` leaves a field as it is; for a field
/// that may be empty, `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct StatusChanges {
    pub label: Option<String>,
    pub description: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub position: Option<i32>,
    pub is_default: Option<bool>,
    pub applies_to_kinds: Option<Option<Vec<String>>>,
}

fn clean(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Picker order: by category as `AssetStatus::ALL` lists them, then
/// position, then id.
fn sort(statuses: &mut [AssetNamedStatus]) {
    let rank = |s: &AssetNamedStatus| {
        AssetStatus::ALL
            .iter()
            .position(|c| c.as_str() == s.category)
            .unwrap_or(AssetStatus::ALL.len())
    };
    statuses.sort_by_key(|s| (rank(s), s.position, s.id));
}

/// The workspace's statuses in picker order, archived ones too when asked.
pub fn list(conn: &mut DbConnection, include_archived: bool) -> QueryResult<Vec<AssetNamedStatus>> {
    let mut query = asset_statuses::table.into_boxed();
    if !include_archived {
        query = query.filter(asset_statuses::archived_at.is_null());
    }
    let mut out: Vec<AssetNamedStatus> = query.load(conn)?;
    sort(&mut out);
    Ok(out)
}

pub fn get(conn: &mut DbConnection, id: i32) -> QueryResult<Option<AssetNamedStatus>> {
    asset_statuses::table.find(id).first(conn).optional()
}

pub fn get_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<Option<AssetNamedStatus>> {
    asset_statuses::table
        .filter(asset_statuses::slug.eq(slug))
        .first(conn)
        .optional()
}

/// The live status named `label`, ignoring case.
pub fn live_by_label(
    conn: &mut DbConnection,
    label: &str,
) -> QueryResult<Option<AssetNamedStatus>> {
    asset_statuses::table
        .filter(asset_statuses::archived_at.is_null())
        .filter(
            diesel::dsl::sql::<diesel::sql_types::Bool>("lower(label) = lower(")
                .bind::<diesel::sql_types::Text, _>(label.trim())
                .sql(")"),
        )
        .first(conn)
        .optional()
}

/// The statuses with these ids, by id.
pub fn by_ids(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<HashMap<i32, AssetNamedStatus>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(asset_statuses::table
        .filter(asset_statuses::id.eq_any(ids))
        .load::<AssetNamedStatus>(conn)?
        .into_iter()
        .map(|s| (s.id, s))
        .collect())
}

/// How many assets hold each status that any hold.
pub fn asset_counts(conn: &mut DbConnection) -> QueryResult<Vec<(i32, i64)>> {
    let rows: Vec<(Option<i32>, i64)> = assets::table
        .filter(assets::status_id.is_not_null())
        .group_by(assets::status_id)
        .select((assets::status_id, diesel::dsl::count_star()))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, n)| id.map(|id| (id, n)))
        .collect())
}

/// How many rows hold status `id`: assets, lifecycle events (either side)
/// and loans.
pub fn usage(conn: &mut DbConnection, id: i32) -> QueryResult<i64> {
    let assets: i64 = assets::table
        .filter(assets::status_id.eq(id))
        .count()
        .get_result(conn)?;
    let events: i64 = asset_lifecycle_events::table
        .filter(
            asset_lifecycle_events::from_status_id
                .eq(id)
                .or(asset_lifecycle_events::to_status_id.eq(id)),
        )
        .count()
        .get_result(conn)?;
    let loans: i64 = asset_loans::table
        .filter(asset_loans::status_id_before.eq(id))
        .count()
        .get_result(conn)?;
    Ok(assets + events + loans)
}

fn check_category(category: &str) -> Result<AssetStatus, StatusError> {
    AssetStatus::from_str_opt(category)
        .ok_or_else(|| StatusError::UnknownCategory(category.to_string()))
}

fn check_label(label: Option<String>) -> Result<String, StatusError> {
    let label = clean(label).ok_or(StatusError::LabelRequired)?;
    if label.chars().count() > LABEL_MAX {
        return Err(StatusError::TooLong {
            field: "label",
            max: LABEL_MAX,
        });
    }
    if names_a_category(&label) {
        return Err(StatusError::LabelReserved);
    }
    Ok(label)
}

/// Whether `label` reads as a category: its key or its name ("In repair"
/// is `in_repair` with a space), ignoring case, spacing, `_` and `-`. An
/// import file writes a plain asset's status as the key, so a named status
/// called the same would take its rows.
fn names_a_category(label: &str) -> bool {
    let loose = |s: &str| {
        s.to_lowercase()
            .replace(['_', '-'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let wanted = loose(label);
    AssetStatus::ALL.iter().any(|c| loose(c.as_str()) == wanted)
}

fn check_color(color: Option<&str>) -> Result<(), StatusError> {
    match color {
        Some(c) if !COLOR.is_match(c) => Err(StatusError::InvalidColor(c.to_string())),
        _ => Ok(()),
    }
}

fn check_kinds(conn: &mut DbConnection, kinds: &[String]) -> Result<(), StatusError> {
    let known: HashSet<String> = asset_kinds::table
        .filter(asset_kinds::slug.eq_any(kinds))
        .select(asset_kinds::slug)
        .load::<String>(conn)?
        .into_iter()
        .collect();
    match kinds.iter().find(|k| !known.contains(*k)) {
        Some(unknown) => Err(StatusError::UnknownKind(unknown.clone())),
        None => Ok(()),
    }
}

/// Whether another live status than `except` has `label`, ignoring case.
fn label_taken(conn: &mut DbConnection, label: &str, except: Option<i32>) -> QueryResult<bool> {
    Ok(live_by_label(conn, label)?.is_some_and(|s| Some(s.id) != except))
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Take the default flag from `category`'s live default, other than
/// `except`, so another status can have it.
fn clear_default(
    conn: &mut DbConnection,
    category: &str,
    except: Option<i32>,
) -> QueryResult<usize> {
    let mut query = diesel::update(asset_statuses::table)
        .filter(asset_statuses::category.eq(category))
        .filter(asset_statuses::is_default.eq(true))
        .filter(asset_statuses::archived_at.is_null())
        .into_boxed();
    if let Some(id) = except {
        query = query.filter(asset_statuses::id.ne(id));
    }
    query
        .set((
            asset_statuses::is_default.eq(false),
            asset_statuses::updated_at.eq(Utc::now()),
        ))
        .execute(conn)
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Create a status. As the default, it takes the flag from the category's
/// current default.
pub fn create(
    conn: &mut DbConnection,
    input: NewStatusInput,
) -> Result<AssetNamedStatus, StatusError> {
    let category = check_category(input.category.trim())?;
    let slug = input.slug.trim().to_string();
    if !KEY.is_match(&slug) {
        return Err(StatusError::InvalidKey(slug));
    }
    if AssetStatus::from_str_opt(&slug).is_some() {
        return Err(StatusError::SlugReserved);
    }
    let label = check_label(Some(input.label))?;
    let color = clean(input.color);
    check_color(color.as_deref())?;
    if input.is_default && input.applies_to_kinds.is_some() {
        return Err(StatusError::DefaultLimitedToKinds);
    }
    if let Some(kinds) = &input.applies_to_kinds {
        check_kinds(conn, kinds)?;
    }
    conn.transaction::<_, StatusError, _>(|conn| {
        if get_by_slug(conn, &slug)?.is_some() {
            return Err(StatusError::SlugTaken);
        }
        if label_taken(conn, &label, None)? {
            return Err(StatusError::LabelTaken);
        }
        if input.is_default {
            clear_default(conn, category.as_str(), None)?;
        }
        Ok(diesel::insert_into(asset_statuses::table)
            .values(&NewAssetNamedStatus {
                category: category.as_str().to_string(),
                slug,
                label,
                description: clean(input.description),
                color,
                position: input.position,
                is_default: input.is_default,
                applies_to_kinds: input.applies_to_kinds,
                created_by: input.created_by,
            })
            .get_result(conn)?)
    })
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Edit status `id`. Its category and slug never change.
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    changes: StatusChanges,
) -> Result<AssetNamedStatus, StatusError> {
    conn.transaction::<_, StatusError, _>(|conn| {
        let current: AssetNamedStatus = asset_statuses::table
            .find(id)
            .for_no_key_update()
            .first(conn)
            .optional()?
            .ok_or(StatusError::NotFound)?;
        let label = match changes.label {
            Some(label) => Some(check_label(Some(label))?),
            None => None,
        };
        let color = changes.color.map(clean);
        check_color(color.as_ref().and_then(|c| c.as_deref()))?;
        let is_default = changes.is_default.unwrap_or(current.is_default);
        let kinds_after = match &changes.applies_to_kinds {
            Some(kinds) => kinds.is_some(),
            None => current.applies_to_kinds.is_some(),
        };
        if is_default && kinds_after {
            return Err(StatusError::DefaultLimitedToKinds);
        }
        if let Some(Some(kinds)) = &changes.applies_to_kinds {
            check_kinds(conn, kinds)?;
        }
        if current.archived_at.is_none() {
            if let Some(label) = &label {
                if label_taken(conn, label, Some(id))? {
                    return Err(StatusError::LabelTaken);
                }
            }
            if changes.is_default == Some(true) && !current.is_default {
                clear_default(conn, &current.category, Some(id))?;
            }
        }
        Ok(diesel::update(asset_statuses::table.find(id))
            .set(&AssetNamedStatusChange {
                label,
                description: changes.description.map(clean),
                color,
                position: changes.position,
                is_default: changes.is_default,
                applies_to_kinds: changes.applies_to_kinds,
                archived_at: None,
                updated_at: Some(Utc::now()),
            })
            .get_result(conn)?)
    })
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Order `category`'s statuses as `ids` lists them; one it leaves out goes
/// after, in its current order.
pub fn reorder(
    conn: &mut DbConnection,
    category: &str,
    ids: &[i32],
) -> Result<Vec<AssetNamedStatus>, StatusError> {
    let category = check_category(category)?;
    conn.transaction::<_, StatusError, _>(|conn| {
        let mut current: Vec<AssetNamedStatus> = asset_statuses::table
            .filter(asset_statuses::category.eq(category.as_str()))
            .for_no_key_update()
            .load(conn)?;
        sort(&mut current);
        let known: HashSet<i32> = current.iter().map(|s| s.id).collect();
        if let Some(stray) = ids.iter().find(|id| !known.contains(id)) {
            return Err(StatusError::OtherCategory {
                category: get(conn, *stray)?
                    .map(|s| s.category)
                    .ok_or(StatusError::NotFound)?,
            });
        }
        let mut seen = HashSet::new();
        let order: Vec<i32> = ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .chain(current.iter().map(|s| s.id).filter(|id| !ids.contains(id)))
            .collect();
        for (position, id) in order.iter().enumerate() {
            let position = i32::try_from(position).unwrap_or(i32::MAX);
            if current
                .iter()
                .any(|s| s.id == *id && s.position != position)
            {
                diesel::update(asset_statuses::table.find(*id))
                    .set((
                        asset_statuses::position.eq(position),
                        asset_statuses::updated_at.eq(Utc::now()),
                    ))
                    .execute(conn)?;
            }
        }
        let mut out: Vec<AssetNamedStatus> = asset_statuses::table
            .filter(asset_statuses::category.eq(category.as_str()))
            .load(conn)?;
        sort(&mut out);
        Ok(out)
    })
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Archive status `id`: the rows holding it keep it, and nothing moves to
/// it. An archived default no longer names its category.
pub fn archive(conn: &mut DbConnection, id: i32) -> Result<AssetNamedStatus, StatusError> {
    let current = get(conn, id)?.ok_or(StatusError::NotFound)?;
    if current.archived_at.is_some() {
        return Ok(current);
    }
    Ok(diesel::update(asset_statuses::table.find(id))
        .set((
            asset_statuses::archived_at.eq(Some(Utc::now())),
            asset_statuses::updated_at.eq(Utc::now()),
        ))
        .get_result(conn)?)
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Bring archived status `id` back. Refused when a live status took its
/// name meanwhile; a default comes back as one only when its category has
/// none.
pub fn restore(conn: &mut DbConnection, id: i32) -> Result<AssetNamedStatus, StatusError> {
    conn.transaction::<_, StatusError, _>(|conn| {
        let current: AssetNamedStatus = asset_statuses::table
            .find(id)
            .for_no_key_update()
            .first(conn)
            .optional()?
            .ok_or(StatusError::NotFound)?;
        if current.archived_at.is_none() {
            return Ok(current);
        }
        if label_taken(conn, &current.label, Some(id))? {
            return Err(StatusError::LabelTaken);
        }
        let default_taken = current.is_default
            && asset_statuses::table
                .filter(asset_statuses::category.eq(&current.category))
                .filter(asset_statuses::is_default.eq(true))
                .filter(asset_statuses::archived_at.is_null())
                .filter(asset_statuses::id.ne(id))
                .count()
                .get_result::<i64>(conn)?
                > 0;
        Ok(diesel::update(asset_statuses::table.find(id))
            .set(&AssetNamedStatusChange {
                is_default: default_taken.then_some(false),
                archived_at: Some(None),
                updated_at: Some(Utc::now()),
                ..Default::default()
            })
            .get_result(conn)?)
    })
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// Delete status `id`, only when no asset, lifecycle event or loan holds
/// it; archive the others.
pub fn delete(conn: &mut DbConnection, id: i32) -> Result<(), StatusError> {
    get(conn, id)?.ok_or(StatusError::NotFound)?;
    let count = usage(conn, id)?;
    if count > 0 {
        return Err(StatusError::InUse { count });
    }
    match diesel::delete(asset_statuses::table.find(id)).execute(conn) {
        Ok(_) => Ok(()),
        // A row took the status since the count.
        Err(Error::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            Err(StatusError::InUse { count: 1 })
        }
        Err(e) => Err(e.into()),
    }
}

/// A slug from `label`: lower case letters, digits and underscores,
/// starting with a letter.
fn slug_of(label: &str) -> String {
    let mut slug = String::new();
    for c in label.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('_') && !slug.is_empty() {
            slug.push('_');
        }
    }
    let slug = slug.trim_end_matches('_');
    let slug = if slug.starts_with(|c: char| c.is_ascii_lowercase())
        && AssetStatus::from_str_opt(slug).is_none()
    {
        slug.to_string()
    } else {
        format!("status_{slug}")
    };
    slug.chars()
        .take(56)
        .collect::<String>()
        .trim_end_matches('_')
        .to_string()
}

// sync-audit-only: named statuses are workspace configuration read over REST, audited by tr_audit_asset_statuses
/// The live status named `label` in `category`, created when there is none,
/// for importers that bring another system's status names. Refused when a
/// live status of another category has the name.
pub fn ensure(
    conn: &mut DbConnection,
    category: AssetStatus,
    label: &str,
    created_by: Option<Uuid>,
) -> Result<AssetNamedStatus, StatusError> {
    let label = check_label(Some(label.to_string()))?;
    if let Some(found) = live_by_label(conn, &label)? {
        if found.category == category.as_str() {
            return Ok(found);
        }
        return Err(StatusError::OtherCategory {
            category: found.category,
        });
    }
    let base = slug_of(&label);
    let mut slug = base.clone();
    let mut n = 2;
    while get_by_slug(conn, &slug)?.is_some() {
        slug = format!("{base}_{n}");
        n += 1;
    }
    let position: i32 = asset_statuses::table
        .filter(asset_statuses::category.eq(category.as_str()))
        .select(diesel::dsl::max(asset_statuses::position))
        .first::<Option<i32>>(conn)?
        .map_or(0, |p| p.saturating_add(1));
    create(
        conn,
        NewStatusInput {
            category: category.as_str().to_string(),
            slug,
            label,
            description: None,
            color: None,
            position,
            is_default: false,
            applies_to_kinds: None,
            created_by,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_comes_from_the_label() {
        assert_eq!(slug_of("Awaiting parts"), "awaiting_parts");
        assert_eq!(slug_of("  At vendor (RMA) "), "at_vendor_rma");
        assert_eq!(slug_of("3rd party"), "status_3rd_party");
        assert_eq!(slug_of("!!!"), "status");
        assert_eq!(slug_of("In repair"), "status_in_repair");
    }

    #[test]
    fn a_name_that_reads_as_a_category_is_refused() {
        for label in ["Lost", "in_repair", "  On-Loan ", "IN SERVICE"] {
            assert!(
                matches!(
                    check_label(Some(label.to_string())),
                    Err(StatusError::LabelReserved)
                ),
                "{label}"
            );
        }
        for label in ["Lost in transit", "Repair bench", "Deployed"] {
            assert!(check_label(Some(label.to_string())).is_ok(), "{label}");
        }
        assert!(KEY.is_match(&slug_of(&"x".repeat(200))));
    }
}
