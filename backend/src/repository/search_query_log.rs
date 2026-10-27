//! Search query log repository.
//!
//! The log behind failed-search detection. The search handler writes one row
//! per documentation-scoped search, with the person who ran it; the detector
//! reads it back as the searches people settled on.
//!
//! Search runs as you type, so one search reaches the log as a chain of
//! requests ("vp", "vpn", "vpn se", "vpn setup"). A row counts only when it
//! ends its chain: the same person's next search, if it came within
//! [`CHAIN_WINDOW_SECS`], didn't carry on from it. Carrying on is extending
//! the query, repeating it, or trimming its end while keeping at least half
//! of it (a typo fixed). Trimming more than that, or typing something else,
//! starts a new search. Rows logged before searches carried a person (NULL
//! `user_uuid`) are read as one person's, which is how the duplicate rows the
//! old client sent for each keystroke collapse into one.
//!
//! The rule is applied where the log is read, not where it's written, so it
//! holds for every client and for rows already logged, and it can change
//! without rewriting the log.

use crate::db::DbConnection;
use crate::models::NewSearchQueryLog;
use crate::schema::search_query_log;
use diesel::prelude::*;
use diesel::result::Error;
use diesel::sql_types::{Array, BigInt, Int8, Nullable, Text, Timestamptz};
use uuid::Uuid;

/// The shortest query that says what someone was looking for, in characters.
/// One character is a search being started, never a topic. Two can be one
/// in IT ("IP", "AD", "PC"), so two count.
pub const MIN_QUERY_CHARS: usize = 2;

/// How soon a person's next search has to follow for it to carry the last
/// one on. Debounced search-as-you-type sends a request every pause of a
/// fraction of a second; ten seconds covers a slow typist and a quick
/// refinement, while a search typed after reading the results starts anew.
pub const CHAIN_WINDOW_SECS: i64 = 10;

/// Lower-case + collapse internal whitespace. Matches what a human
/// would consider "the same query" without re-implementing the
/// search engine's stemming.
pub fn normalise_query(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase()
}

/// Whether a normalised query can be demand for a doc. Not one shorter than
/// [`MIN_QUERY_CHARS`], and not a ticket number ("83", "#83"): search finds
/// the ticket by its number, so that's a lookup, not a question.
pub fn is_demand(query_norm: &str) -> bool {
    let digits = query_norm.trim_start_matches('#');
    let ticket_number = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    query_norm.chars().count() >= MIN_QUERY_CHARS && !ticket_number
}

// sync-audit-only: Operational / bespoke tables
/// Insert one log row for a search `user_uuid` ran. A query that can't be
/// demand for a doc (see [`is_demand`]) isn't logged. Errors are non-fatal
/// at the call site: search must succeed even if logging fails. Timed by the
/// app's clock, not the column's `now()`: `count_failed_since` compares it
/// with gap close times, which the app stamps.
pub fn log_query(
    conn: &mut DbConnection,
    user_uuid: Option<Uuid>,
    query_raw: &str,
    result_count: i32,
) -> Result<(), Error> {
    let query_norm = normalise_query(query_raw);
    if !is_demand(&query_norm) {
        return Ok(());
    }
    let row = NewSearchQueryLog {
        query_raw: query_raw.trim().to_string(),
        query_norm,
        result_count,
        searched_at: chrono::Utc::now().naive_utc(),
        user_uuid,
    };
    diesel::insert_into(search_query_log::table)
        .values(&row)
        .execute(conn)?;
    Ok(())
}

/// One aggregated row per recurring zero-result query. The
/// detector turns these into `failed_search` signals.
#[derive(Debug, Clone, QueryableByName)]
pub struct FailedSearchAggregate {
    #[diesel(sql_type = Text)]
    pub query_norm: String,
    /// The query as it was last typed (case and punctuation kept), for the UI.
    #[diesel(sql_type = Text)]
    pub query_sample: String,
    /// Settled searches for it that found no documentation.
    #[diesel(sql_type = BigInt)]
    pub count: i64,
    /// How many people ran them. A row from before searches carried a person
    /// counts as one person.
    #[diesel(sql_type = BigInt)]
    pub people: i64,
    #[diesel(sql_type = Timestamptz)]
    pub first_seen: chrono::NaiveDateTime,
    #[diesel(sql_type = Timestamptz)]
    pub last_seen: chrono::NaiveDateTime,
}

// sync-audit-only: read-only aggregation over the search log (sql_query), no writes
/// Settled searches since `since` that found no documentation, grouped by
/// normalised query, with at least `min_count` searches each. `only` limits
/// it to those queries. Ordered by count, most first. See the module docs
/// for what counts as settled.
pub fn failed_searches(
    conn: &mut DbConnection,
    since: chrono::NaiveDateTime,
    min_count: i64,
    only: Option<&[String]>,
) -> Result<Vec<FailedSearchAggregate>, Error> {
    // The chain is read across every query a person ran, so a row is judged
    // by whatever they searched next, whether or not it found anything. Too
    // short and ticket-number queries are left out first: "v" typed after
    // clearing "vpn" doesn't carry "vpn" on.
    let sql = format!(
        "WITH logged AS ( \
             SELECT query_norm, query_raw, result_count, searched_at, user_uuid, \
                    LEAD(query_norm) OVER chain AS next_norm, \
                    LEAD(searched_at) OVER chain AS next_at \
             FROM search_query_log \
             WHERE searched_at >= $1 \
               AND char_length(query_norm) >= {min_chars} \
               AND query_norm !~ '^#?[0-9]+$' \
             WINDOW chain AS (PARTITION BY workspace_id, user_uuid ORDER BY searched_at, id) \
         ), \
         settled AS ( \
             SELECT * FROM logged \
             WHERE next_at IS NULL \
                OR next_at - searched_at > make_interval(secs => {window}) \
                OR NOT ( \
                    starts_with(next_norm, query_norm) \
                    OR (starts_with(query_norm, next_norm) \
                        AND char_length(next_norm) * 2 >= char_length(query_norm)) \
                ) \
         ) \
         SELECT query_norm, \
                (array_agg(query_raw ORDER BY searched_at DESC))[1] AS query_sample, \
                COUNT(*) AS count, \
                COUNT(DISTINCT user_uuid) + COUNT(*) FILTER (WHERE user_uuid IS NULL) AS people, \
                MIN(searched_at) AS first_seen, \
                MAX(searched_at) AS last_seen \
         FROM settled \
         WHERE result_count = 0 AND ($3::text[] IS NULL OR query_norm = ANY($3)) \
         GROUP BY query_norm \
         HAVING COUNT(*) >= $2 \
         ORDER BY count DESC, people DESC, query_norm",
        min_chars = MIN_QUERY_CHARS,
        window = CHAIN_WINDOW_SECS,
    );
    diesel::sql_query(sql)
        .bind::<Timestamptz, _>(since)
        .bind::<Int8, _>(min_count)
        .bind::<Nullable<Array<Text>>, _>(only.map(<[String]>::to_vec))
        .load(conn)
}

/// Queries settled on and found nothing at least `min_count` times in the
/// last `days`. Returns aggregates sorted by count desc.
pub fn aggregate_failed_searches(
    conn: &mut DbConnection,
    days: i32,
    min_count: i64,
) -> Result<Vec<FailedSearchAggregate>, Error> {
    let since = chrono::Utc::now().naive_utc() - chrono::Duration::days(days as i64);
    failed_searches(conn, since, min_count, None)
}

/// How often a normalised query was settled on and found nothing since
/// `since`. The detector uses it to tell a gap closed earlier from demand
/// that came after.
pub fn count_failed_since(
    conn: &mut DbConnection,
    query_norm: &str,
    since: chrono::NaiveDateTime,
) -> Result<i64, Error> {
    let only = [query_norm.to_string()];
    Ok(failed_searches(conn, since, 1, Some(&only))?
        .first()
        .map_or(0, |a| a.count))
}

/// Which of `query_norms` the log holds any row for, settled or not.
pub fn logged_queries(
    conn: &mut DbConnection,
    query_norms: &[String],
) -> Result<std::collections::HashSet<String>, Error> {
    Ok(search_query_log::table
        .filter(search_query_log::query_norm.eq_any(query_norms))
        .select(search_query_log::query_norm)
        .distinct()
        .load::<String>(conn)?
        .into_iter()
        .collect())
}

// sync-audit-only: Operational / bespoke tables
/// Drop log rows older than `retention_days`. Run periodically by
/// the scheduler. Returns the number of rows deleted.
pub fn prune_old_rows(conn: &mut DbConnection, retention_days: i32) -> Result<usize, Error> {
    let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::days(retention_days as i64);
    diesel::delete(search_query_log::table.filter(search_query_log::searched_at.lt(cutoff)))
        .execute(conn)
}
