//! Search query log repository.
//!
//! Append-only log feeding Phase 2c's failed-search detection.
//! `log_query` writes one row per search; `aggregate_failed_searches`
//! groups zero-result queries by their normalised form for the
//! detector. Retention is handled by a periodic delete sweep.

use crate::db::DbConnection;
use crate::models::NewSearchQueryLog;
use crate::schema::search_query_log;
use diesel::prelude::*;
use diesel::result::Error;

/// Lower-case + collapse internal whitespace. Matches what a human
/// would consider "the same query" without re-implementing the
/// search engine's stemming.
pub fn normalise_query(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase()
}

/// Insert one log row. Errors are non-fatal at the call site —
/// search must succeed even if logging fails.
pub fn log_query(conn: &mut DbConnection, query_raw: &str, result_count: i32) -> Result<(), Error> {
    let query_norm = normalise_query(query_raw);
    if query_norm.is_empty() {
        return Ok(());
    }
    let row = NewSearchQueryLog {
        query_raw: query_raw.to_string(),
        query_norm,
        result_count,
    };
    diesel::insert_into(search_query_log::table)
        .values(&row)
        .execute(conn)?;
    Ok(())
}

/// One aggregated row per recurring zero-result query. The
/// detector turns these into `failed_search` signals.
#[derive(Debug, Clone)]
pub struct FailedSearchAggregate {
    pub query_norm: String,
    pub query_sample: String,
    pub count: i64,
    pub first_seen: chrono::NaiveDateTime,
    pub last_seen: chrono::NaiveDateTime,
}

/// Find zero-result queries that recurred at least `min_count`
/// times in the window. Returns aggregates sorted by count desc.
pub fn aggregate_failed_searches(
    conn: &mut DbConnection,
    days: i32,
    min_count: i64,
) -> Result<Vec<FailedSearchAggregate>, Error> {
    use diesel::dsl::{count_star, max, min};

    let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::days(days as i64);

    let rows: Vec<(
        String,
        i64,
        Option<chrono::NaiveDateTime>,
        Option<chrono::NaiveDateTime>,
    )> = search_query_log::table
        .filter(search_query_log::result_count.eq(0))
        .filter(search_query_log::searched_at.ge(cutoff))
        .group_by(search_query_log::query_norm)
        .having(count_star().ge(min_count))
        .select((
            search_query_log::query_norm,
            count_star(),
            min(search_query_log::searched_at),
            max(search_query_log::searched_at),
        ))
        .order_by(count_star().desc())
        .load(conn)?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Sample one raw form per normalised key so the UI can render
    // what the user actually typed (case + punctuation), not the
    // lowered/collapsed version.
    use std::collections::HashMap;
    let norms: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let samples: HashMap<String, String> = search_query_log::table
        .filter(search_query_log::query_norm.eq_any(&norms))
        .filter(search_query_log::result_count.eq(0))
        .select((search_query_log::query_norm, search_query_log::query_raw))
        .load::<(String, String)>(conn)?
        .into_iter()
        .fold(HashMap::new(), |mut acc, (norm, raw)| {
            acc.entry(norm).or_insert(raw);
            acc
        });

    Ok(rows
        .into_iter()
        .filter_map(|(query_norm, count, first, last)| {
            let (first_seen, last_seen) = (first?, last?);
            let query_sample = samples
                .get(&query_norm)
                .cloned()
                .unwrap_or_else(|| query_norm.clone());
            Some(FailedSearchAggregate {
                query_norm,
                query_sample,
                count,
                first_seen,
                last_seen,
            })
        })
        .collect())
}

/// Drop log rows older than `retention_days`. Run periodically by
/// the scheduler. Returns the number of rows deleted.
pub fn prune_old_rows(conn: &mut DbConnection, retention_days: i32) -> Result<usize, Error> {
    let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::days(retention_days as i64);
    diesel::delete(search_query_log::table.filter(search_query_log::searched_at.lt(cutoff)))
        .execute(conn)
}
