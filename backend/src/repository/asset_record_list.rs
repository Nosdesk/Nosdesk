//! The workspace's asset records across every asset, for the Records tab:
//! a page of the filtered set, what the whole set adds up to, its groups,
//! the vendors and assets its filters offer, and the record text an asset's
//! search document carries.
//!
//! One filter builds every query here, so the page, the totals, the groups
//! and the export always describe the same set. Costs are summed per
//! currency and never converted. RLS scopes every read to the workspace.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::query_builder::{BoxedSqlQuery, SqlQuery};
use diesel::serialize::ToSql;
use diesel::sql_types::{Array, BigInt, Date, HasSqlType, Integer, Nullable, Text};
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::AssetRecord;
use crate::schema::{asset_records, assets};

/// Which records the list holds. Every set filter must hold; a list filter
/// that is empty is not set.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    /// Text in the summary, vendor or reference, or the asset's tag or name.
    pub q: Option<String>,
    /// Records of these types. `Some(vec![])` matches nothing: the types
    /// asked for don't exist.
    pub record_type_ids: Option<Vec<i32>>,
    /// `Some(true)` for open work only, `Some(false)` for completed only.
    pub open: Option<bool>,
    /// Vendors, matched without regard to case or surrounding space.
    pub vendors: Vec<String>,
    pub asset_ids: Vec<i32>,
    /// Work done on or after this day.
    pub from: Option<NaiveDate>,
    /// Work done on or before this day.
    pub to: Option<NaiveDate>,
    /// Only these records.
    pub ids: Vec<i32>,
}

/// What the list is sorted by. Ties fall to the newest work first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Date,
    Cost,
    Asset,
    Vendor,
}

impl Sort {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "date" => Some(Self::Date),
            "cost" => Some(Self::Cost),
            "asset" => Some(Self::Asset),
            "vendor" => Some(Self::Vendor),
            _ => None,
        }
    }
}

/// What the list is grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupAxis {
    Vendor,
    Type,
    Asset,
    Month,
}

impl GroupAxis {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "vendor" => Some(Self::Vendor),
            "type" => Some(Self::Type),
            "asset" => Some(Self::Asset),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vendor => "vendor",
            Self::Type => "type",
            Self::Asset => "asset",
            Self::Month => "month",
        }
    }

    /// The group a row falls in, as text: the vendor in lower case (empty
    /// for work done in house), the type's id, the asset's id, or the month
    /// of the work as `YYYY-MM`.
    fn key_sql(self) -> &'static str {
        match self {
            Self::Vendor => "COALESCE(lower(btrim(r.vendor)), '')",
            Self::Type => "r.record_type_id::text",
            Self::Asset => "r.asset_id::text",
            Self::Month => "to_char(r.occurred_on, 'YYYY-MM')",
        }
    }
}

/// How many records, how many still open, and their costs per currency.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetTotals {
    pub count: i64,
    pub open: i64,
    /// Minor units per currency code.
    pub costs: BTreeMap<String, i64>,
}

impl SetTotals {
    fn add(&mut self, row: &TotalsRow) {
        self.count += row.records;
        self.open += row.open;
        if let (Some(code), Some(minor)) = (row.currency.as_deref(), row.cost_minor) {
            let sum = self.costs.entry(code.to_string()).or_insert(0);
            *sum = sum.saturating_add(minor);
        }
    }

    /// The sum in `currency`, zero when there is none.
    fn spend_in(&self, currency: Option<&str>) -> i64 {
        currency
            .and_then(|code| self.costs.get(code))
            .copied()
            .unwrap_or(0)
    }
}

/// One group of the filtered set and what it adds up to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// As [`GroupAxis`] keys rows.
    pub key: String,
    /// The vendor's most used spelling, for a vendor group with one.
    pub label: Option<String>,
    pub totals: SetTotals,
}

/// A record in the list and the asset it belongs to.
#[derive(Debug, Clone)]
pub struct ListRow {
    pub record: AssetRecord,
    pub asset: AssetBrief,
    /// The row's group, when the list is grouped.
    pub group: Option<String>,
}

/// The parts of an asset a list row names it by.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AssetBrief {
    pub id: i32,
    pub name: String,
    pub asset_tag: Option<String>,
}

/// A vendor written on records, under its most used spelling.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VendorCount {
    pub vendor: String,
    pub count: i64,
}

/// An asset that has records.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AssetCount {
    pub asset: AssetBrief,
    pub count: i64,
}

/// What the list's filters offer: the vendors and assets on records, most
/// used first, and how many records the workspace has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub total: i64,
    pub vendors: Vec<VendorCount>,
    pub assets: Vec<AssetCount>,
}

const FROM: &str = " FROM asset_records r JOIN assets a ON a.id = r.asset_id WHERE TRUE";

/// Raw SQL built a piece at a time, numbering its placeholders as values
/// are bound (a boxed `sql_query` binds values only).
struct Query<'a> {
    query: BoxedSqlQuery<'a, Pg, SqlQuery>,
    binds: usize,
}

impl<'a> Query<'a> {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            query: diesel::sql_query(sql).into_boxed(),
            binds: 0,
        }
    }

    fn sql(mut self, sql: impl AsRef<str>) -> Self {
        self.query = self.query.sql(sql);
        self
    }

    /// Write a placeholder for `value` and bind it.
    fn bind<ST, V>(mut self, value: V) -> Self
    where
        Pg: HasSqlType<ST>,
        V: ToSql<ST, Pg> + Send + 'a,
        ST: Send + 'a,
    {
        self.binds += 1;
        self.query = self
            .query
            .sql(format!("${}", self.binds))
            .bind::<ST, V>(value);
        self
    }

    fn load<T: QueryableByName<Pg> + 'static>(
        self,
        conn: &mut DbConnection,
    ) -> QueryResult<Vec<T>> {
        self.query.load(conn)
    }
}

/// `query` narrowed to `filter`'s records.
fn filtered<'a>(mut query: Query<'a>, filter: &ListFilter) -> Query<'a> {
    if let Some(text) = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        let pattern = format!("%{}%", crate::repository::escape_like(text));
        query = query.sql(" AND (r.summary ILIKE ");
        query = query.bind::<Text, _>(pattern.clone());
        for column in ["r.vendor", "r.reference", "a.asset_tag", "a.name"] {
            query = query.sql(format!(" OR {column} ILIKE "));
            query = query.bind::<Text, _>(pattern.clone());
        }
        query = query.sql(")");
    }
    if let Some(ids) = &filter.record_type_ids {
        query = query.sql(" AND r.record_type_id = ANY(");
        query = query.bind::<Array<Integer>, _>(ids.clone());
        query = query.sql(")");
    }
    match filter.open {
        Some(true) => query = query.sql(" AND r.completed_on IS NULL"),
        Some(false) => query = query.sql(" AND r.completed_on IS NOT NULL"),
        None => {}
    }
    if !filter.vendors.is_empty() {
        // Postgres folds both sides, so a vendor compares as its group's key.
        query = query.sql(" AND lower(btrim(r.vendor)) IN (SELECT lower(btrim(v)) FROM unnest(");
        query = query.bind::<Array<Text>, _>(filter.vendors.clone());
        query = query.sql("::text[]) AS v)");
    }
    if !filter.asset_ids.is_empty() {
        query = query.sql(" AND r.asset_id = ANY(");
        query = query.bind::<Array<Integer>, _>(filter.asset_ids.clone());
        query = query.sql(")");
    }
    if let Some(from) = filter.from {
        query = query.sql(" AND r.occurred_on >= ");
        query = query.bind::<Date, _>(from);
    }
    if let Some(to) = filter.to {
        query = query.sql(" AND r.occurred_on <= ");
        query = query.bind::<Date, _>(to);
    }
    if !filter.ids.is_empty() {
        query = query.sql(" AND r.id = ANY(");
        query = query.bind::<Array<Integer>, _>(filter.ids.clone());
        query = query.sql(")");
    }
    query
}

#[derive(QueryableByName)]
struct TotalsRow {
    #[diesel(sql_type = Text)]
    key: String,
    #[diesel(sql_type = Nullable<Text>)]
    label: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    currency: Option<String>,
    #[diesel(sql_type = BigInt)]
    records: i64,
    #[diesel(sql_type = BigInt)]
    open: i64,
    #[diesel(sql_type = Nullable<BigInt>)]
    cost_minor: Option<i64>,
}

/// Counts and costs of `filter`'s records per group key (all one group
/// without an axis) and currency.
fn totals_rows(
    conn: &mut DbConnection,
    filter: &ListFilter,
    axis: Option<GroupAxis>,
) -> QueryResult<Vec<TotalsRow>> {
    let key = axis.map_or("''", GroupAxis::key_sql);
    let label = match axis {
        Some(GroupAxis::Vendor) => "mode() WITHIN GROUP (ORDER BY btrim(r.vendor))",
        _ => "NULL::text",
    };
    let select = format!(
        "SELECT {key} AS key, {label} AS label, r.cost_currency::text AS currency, \
                COUNT(*)::int8 AS records, \
                (COUNT(*) FILTER (WHERE r.completed_on IS NULL))::int8 AS open, \
                SUM(r.cost_minor)::int8 AS cost_minor"
    );
    let query = Query::new(select).sql(FROM);
    filtered(query, filter).sql(" GROUP BY 1, 3").load(conn)
}

// sync-audit-only: read-only (one grouped SELECT)
/// What every record `filter` matches adds up to, whatever page is loaded.
pub fn totals(conn: &mut DbConnection, filter: &ListFilter) -> QueryResult<SetTotals> {
    let mut out = SetTotals::default();
    for row in totals_rows(conn, filter, None)? {
        out.add(&row);
    }
    Ok(out)
}

// sync-audit-only: read-only (one grouped SELECT)
/// `filter`'s records in groups along `axis`, in the order the list shows
/// them: months newest first; any other axis with the biggest spend in
/// `currency` first, then the most records.
pub fn groups(
    conn: &mut DbConnection,
    filter: &ListFilter,
    axis: GroupAxis,
    currency: Option<&str>,
) -> QueryResult<Vec<Group>> {
    let mut by_key: BTreeMap<String, Group> = BTreeMap::new();
    // The label of a group split across currencies comes from its biggest
    // part.
    let mut label_weight: HashMap<String, i64> = HashMap::new();
    for row in totals_rows(conn, filter, Some(axis))? {
        let group = by_key.entry(row.key.clone()).or_insert_with(|| Group {
            key: row.key.clone(),
            label: None,
            totals: SetTotals::default(),
        });
        group.totals.add(&row);
        let weight = label_weight.entry(row.key.clone()).or_insert(-1);
        if row.label.is_some() && row.records > *weight {
            *weight = row.records;
            group.label = row.label.clone();
        }
    }
    let mut out: Vec<Group> = by_key.into_values().collect();
    match axis {
        GroupAxis::Month => out.sort_by(|a, b| b.key.cmp(&a.key)),
        _ => out.sort_by(|a, b| {
            b.totals
                .spend_in(currency)
                .cmp(&a.totals.spend_in(currency))
                .then_with(|| b.totals.count.cmp(&a.totals.count))
                .then_with(|| a.key.cmp(&b.key))
        }),
    }
    Ok(out)
}

/// The ORDER BY for `sort` in `descending` order or not, ending in the id
/// so the order is total.
fn order_sql(sort: Sort, descending: bool) -> String {
    let dir = if descending { "DESC" } else { "ASC" };
    // Newest work first within a tie, whatever the direction of the key.
    const NEWEST: &str = "r.occurred_on DESC, r.occurred_at DESC NULLS LAST, r.id DESC";
    match sort {
        Sort::Date if descending => NEWEST.to_string(),
        Sort::Date => "r.occurred_on ASC, r.occurred_at ASC NULLS FIRST, r.id ASC".to_string(),
        Sort::Cost => format!("r.cost_minor {dir} NULLS LAST, {NEWEST}"),
        Sort::Asset => format!(
            "lower(COALESCE(NULLIF(btrim(a.asset_tag), ''), a.name)) {dir}, a.id {dir}, {NEWEST}"
        ),
        Sort::Vendor => format!("lower(btrim(r.vendor)) {dir} NULLS LAST, {NEWEST}"),
    }
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Text>)]
    group_key: Option<String>,
}

// sync-audit-only: read-only (two SELECTs)
/// A page of `filter`'s records sorted by `sort`, with their assets: from
/// `offset`, at most `limit` of them (every one without a limit). Grouped,
/// the rows run group by group in `groups`' order, sorted within each.
pub fn page(
    conn: &mut DbConnection,
    filter: &ListFilter,
    sort: Sort,
    descending: bool,
    grouped: Option<(GroupAxis, &[Group])>,
    offset: i64,
    limit: Option<i64>,
) -> QueryResult<Vec<ListRow>> {
    let key = grouped.map_or("NULL::text", |(axis, _)| axis.key_sql());
    let query = Query::new(format!("SELECT r.id AS id, {key} AS group_key")).sql(FROM);
    let mut query = filtered(query, filter).sql(" ORDER BY ");
    if let Some((axis, groups)) = grouped {
        let keys: Vec<String> = groups.iter().map(|g| g.key.clone()).collect();
        query = query.sql("array_position(");
        query = query.bind::<Array<Text>, _>(keys);
        query = query.sql(format!(", {}), ", axis.key_sql()));
    }
    query = query.sql(order_sql(sort, descending));
    if let Some(limit) = limit {
        query = query.sql(" LIMIT ").bind::<BigInt, _>(limit);
    }
    query = query.sql(" OFFSET ").bind::<BigInt, _>(offset.max(0));
    let ids: Vec<IdRow> = query.load(conn)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let wanted: Vec<i32> = ids.iter().map(|r| r.id).collect();
    let mut records: HashMap<i32, AssetRecord> = asset_records::table
        .filter(asset_records::id.eq_any(&wanted))
        .load::<AssetRecord>(conn)?
        .into_iter()
        .map(|r| (r.id, r))
        .collect();
    let asset_ids: Vec<i32> = records.values().map(|r| r.asset_id).collect();
    let briefs: HashMap<i32, AssetBrief> = assets::table
        .filter(assets::id.eq_any(&asset_ids))
        .select((assets::id, assets::name, assets::asset_tag))
        .load::<(i32, String, Option<String>)>(conn)?
        .into_iter()
        .map(|(id, name, asset_tag)| {
            (
                id,
                AssetBrief {
                    id,
                    name,
                    asset_tag,
                },
            )
        })
        .collect();
    Ok(ids
        .into_iter()
        .filter_map(|row| {
            let record = records.remove(&row.id)?;
            let asset = briefs.get(&record.asset_id)?.clone();
            Some(ListRow {
                record,
                asset,
                group: row.group_key,
            })
        })
        .collect())
}

/// How many vendors and assets the filters offer at most.
const FACET_VENDORS: i64 = 200;
const FACET_ASSETS: i64 = 500;

// sync-audit-only: read-only (three SELECTs)
/// The vendors and assets the list's filters offer, over every record:
/// vendors under their most used spelling, most used first; assets with the
/// most records first.
pub fn facets(conn: &mut DbConnection) -> QueryResult<Facets> {
    #[derive(QueryableByName)]
    struct VendorRow {
        #[diesel(sql_type = Text)]
        vendor: String,
        #[diesel(sql_type = BigInt)]
        records: i64,
    }
    #[derive(QueryableByName)]
    struct AssetRow {
        #[diesel(sql_type = Integer)]
        id: i32,
        #[diesel(sql_type = Text)]
        name: String,
        #[diesel(sql_type = Nullable<Text>)]
        asset_tag: Option<String>,
        #[diesel(sql_type = BigInt)]
        records: i64,
    }

    let total: i64 = asset_records::table.count().get_result(conn)?;
    if total == 0 {
        return Ok(Facets::default());
    }
    let vendors: Vec<VendorRow> = diesel::sql_query(
        "SELECT mode() WITHIN GROUP (ORDER BY btrim(vendor)) AS vendor, \
                COUNT(*)::int8 AS records \
         FROM asset_records \
         WHERE btrim(vendor) <> '' \
         GROUP BY lower(btrim(vendor)) \
         ORDER BY 2 DESC, lower(btrim(vendor)) ASC \
         LIMIT $1",
    )
    .bind::<BigInt, _>(FACET_VENDORS)
    .load(conn)?;
    let assets: Vec<AssetRow> = diesel::sql_query(
        "SELECT a.id, a.name::text AS name, a.asset_tag::text AS asset_tag, \
                COUNT(*)::int8 AS records \
         FROM asset_records r JOIN assets a ON a.id = r.asset_id \
         GROUP BY a.id \
         ORDER BY 4 DESC, a.id ASC \
         LIMIT $1",
    )
    .bind::<BigInt, _>(FACET_ASSETS)
    .load(conn)?;
    Ok(Facets {
        total,
        vendors: vendors
            .into_iter()
            .map(|v| VendorCount {
                vendor: v.vendor,
                count: v.records,
            })
            .collect(),
        assets: assets
            .into_iter()
            .map(|a| AssetCount {
                asset: AssetBrief {
                    id: a.id,
                    name: a.name,
                    asset_tag: a.asset_tag,
                },
                count: a.records,
            })
            .collect(),
    })
}

/// How many of an asset's records its search document reads, newest work
/// first, and how much of their text it carries at most.
const SEARCH_RECORDS: i64 = 200;
const SEARCH_TEXT_BYTES: usize = 16 * 1024;

// sync-audit-only: read-only (one SELECT)
/// The record text each of `asset_ids`' search documents carries: the
/// distinct summaries, vendors and references of its newest
/// [`SEARCH_RECORDS`] records, up to [`SEARCH_TEXT_BYTES`]. `None` reads
/// every asset the connection sees.
pub fn search_text(
    conn: &mut DbConnection,
    asset_ids: Option<&[i32]>,
) -> QueryResult<HashMap<i32, Vec<String>>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Integer)]
        asset_id: i32,
        #[diesel(sql_type = Nullable<Text>)]
        summary: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        vendor: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        reference: Option<String>,
    }

    if asset_ids.is_some_and(<[i32]>::is_empty) {
        return Ok(HashMap::new());
    }
    let mut query = Query::new(
        "SELECT asset_id, summary::text AS summary, vendor::text AS vendor, \
                reference::text AS reference \
         FROM (SELECT asset_id, summary, vendor, reference, \
                      row_number() OVER (PARTITION BY asset_id \
                          ORDER BY occurred_on DESC, id DESC) AS n \
               FROM asset_records",
    );
    if let Some(ids) = asset_ids {
        query = query.sql(" WHERE asset_id = ANY(");
        query = query.bind::<Array<Integer>, _>(ids.to_vec());
        query = query.sql(")");
    }
    query = query.sql(") ranked WHERE n <= ");
    query = query.bind::<BigInt, _>(SEARCH_RECORDS);
    query = query.sql(" ORDER BY asset_id, n");
    let rows: Vec<Row> = query.load(conn)?;

    let mut out: HashMap<i32, Vec<String>> = HashMap::new();
    let mut seen: HashMap<i32, (HashSet<String>, usize)> = HashMap::new();
    for row in rows {
        let texts = out.entry(row.asset_id).or_default();
        let (known, bytes) = seen.entry(row.asset_id).or_default();
        for text in [row.summary, row.vendor, row.reference]
            .into_iter()
            .flatten()
        {
            let text = text.trim();
            if text.is_empty() || *bytes + text.len() > SEARCH_TEXT_BYTES {
                continue;
            }
            if known.insert(text.to_string()) {
                *bytes += text.len();
                texts.push(text.to_string());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_order_ends_in_the_id() {
        for sort in [Sort::Date, Sort::Cost, Sort::Asset, Sort::Vendor] {
            for descending in [true, false] {
                let order = order_sql(sort, descending);
                assert!(order.ends_with("r.id DESC") || order.ends_with("r.id ASC"));
            }
        }
    }

    #[test]
    fn sorts_and_axes_read_their_names() {
        assert_eq!(Sort::parse("cost"), Some(Sort::Cost));
        assert_eq!(Sort::parse("price"), None);
        assert_eq!(GroupAxis::parse("month"), Some(GroupAxis::Month));
        assert_eq!(GroupAxis::parse("none"), None);
    }
}
