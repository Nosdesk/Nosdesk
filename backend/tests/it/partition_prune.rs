//! The scheduled prune drops monthly partitions that lie entirely before the
//! retention cutoff, and nothing else: not the partition the cutoff falls
//! in, not later ones, never the default partition. `sync_actions` and
//! `audit_log` both have a default partition, which rules out DETACH ...
//! CONCURRENTLY (Postgres refuses it while one exists).

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use crate::common::TestDb;

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Partition {
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    bound: String,
}

fn table_exists(conn: &mut PgConnection, name: &str) -> bool {
    diesel::sql_query("SELECT count(*) AS n FROM pg_class WHERE relname = $1")
        .bind::<Text, _>(name)
        .get_result::<Count>(conn)
        .expect("look up pg_class")
        .n
        > 0
}

/// `parent`'s range partitions, each with its upper bound.
fn range_partitions(conn: &mut PgConnection, parent: &str) -> BTreeMap<String, NaiveDate> {
    diesel::sql_query(
        "SELECT c.relname::text AS name, pg_get_expr(c.relpartbound, c.oid) AS bound \
         FROM pg_inherits i \
         JOIN pg_class c ON c.oid = i.inhrelid \
         JOIN pg_class p ON p.oid = i.inhparent \
         WHERE p.relname = $1",
    )
    .bind::<Text, _>(parent)
    .load::<Partition>(conn)
    .expect("list partitions")
    .into_iter()
    .filter_map(|p| {
        let upper = p.bound.split(" TO ('").nth(1)?.get(..10)?;
        Some((
            p.name,
            NaiveDate::parse_from_str(upper, "%Y-%m-%d").expect("upper bound"),
        ))
    })
    .collect()
}

/// The month partition `[from, to)` named the way rotation names it,
/// created unless it already exists.
fn month_partition(conn: &mut PgConnection, parent: &str, month: NaiveDate) -> String {
    let from = NaiveDate::from_ymd_opt(month.year(), month.month(), 1).expect("first of month");
    let to = if from.month() == 12 {
        NaiveDate::from_ymd_opt(from.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(from.year(), from.month() + 1, 1)
    }
    .expect("first of next month");
    let name = format!("{parent}_{}", from.format("%Y_%m"));
    if !table_exists(conn, &name) {
        diesel::sql_query(format!(
            "CREATE TABLE {name} PARTITION OF {parent} FOR VALUES FROM ('{from}') TO ('{to}')"
        ))
        .execute(conn)
        .unwrap_or_else(|e| panic!("create {name}: {e}"));
    }
    name
}

/// Run `prune` on a database holding a long-expired month and the month the
/// cutoff falls in, and check exactly the months entirely before the cutoff
/// went.
async fn prunes_only_expired_months<F, Fut>(parent: &str, retention_days: i64, prune: F)
where
    F: FnOnce(backend::db::Pool) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let db = TestDb::new();
    let mut conn = PgConnection::establish(db.url()).expect("connect");
    let cutoff = Utc::now().date_naive() - Duration::days(retention_days);
    let expired = month_partition(
        &mut conn,
        parent,
        NaiveDate::from_ymd_opt(2020, 1, 1).expect("date"),
    );
    let straddling = month_partition(&mut conn, parent, cutoff);
    let default = format!("{parent}_default");
    assert!(table_exists(&mut conn, &default));
    let before = range_partitions(&mut conn, parent);

    prune(db.pool_with_size(2))
        .await
        .unwrap_or_else(|e| panic!("prune {parent} partitions: {e:#}"));

    let after = range_partitions(&mut conn, parent);
    let kept: BTreeMap<String, NaiveDate> = before
        .into_iter()
        .filter(|(_, upper)| *upper > cutoff)
        .collect();
    assert_eq!(
        after, kept,
        "only months entirely before the {cutoff} cutoff are dropped"
    );
    assert!(!after.contains_key(&expired), "{expired} was dropped");
    assert!(
        after.contains_key(&straddling),
        "{straddling} holds the cutoff"
    );
    assert!(table_exists(&mut conn, &default), "{default} stays");
}

#[actix_web::test]
async fn the_scheduled_prune_drops_expired_sync_actions_months() {
    prunes_only_expired_months(
        "sync_actions",
        90,
        backend::services::scheduled_jobs::prune_sync_actions_partitions,
    )
    .await;
}

#[actix_web::test]
async fn the_scheduled_prune_drops_expired_audit_log_months() {
    prunes_only_expired_months(
        "audit_log",
        540,
        backend::services::scheduled_jobs::prune_audit_log_partitions,
    )
    .await;
}
