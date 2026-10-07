//! Integration tests clone their databases from a template built for this
//! build's migrations, concurrent first use builds it once, and a template
//! another process is using is never dropped as stale.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use diesel::migration::MigrationSource;
use diesel::pg::Pg;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use crate::common::{self, TestDb};

#[derive(QueryableByName)]
struct IsTemplate {
    #[diesel(sql_type = Bool)]
    datistemplate: bool,
}

fn is_template(conn: &mut PgConnection, name: &str) -> Option<bool> {
    diesel::sql_query("SELECT datistemplate FROM pg_database WHERE datname = $1")
        .bind::<Text, _>(name)
        .get_result::<IsTemplate>(conn)
        .optional()
        .expect("look up pg_database")
        .map(|row| row.datistemplate)
}

/// A branch whose migrations differ gets its own template: the name carries
/// the hash `build.rs` takes over `migrations/`, and a sandbox holds exactly
/// this build's migrations, none another branch added and none missing.
#[test]
fn sandboxes_clone_the_template_for_this_builds_migrations() {
    let db = TestDb::new();
    let name = format!("nosdesk_test_template_{}", env!("NOSDESK_SCHEMA_HASH"));
    let mut conn = PgConnection::establish(db.url()).expect("connect to sandbox");
    assert_eq!(
        is_template(&mut conn, &name),
        Some(true),
        "no template `{name}` for this build's migrations"
    );

    let embedded: BTreeSet<String> = MigrationSource::<Pg>::migrations(&backend::db::MIGRATIONS)
        .expect("list embedded migrations")
        .iter()
        .map(|m| m.name().version().to_string())
        .collect();
    let applied: BTreeSet<String> = conn
        .applied_migrations()
        .expect("read the sandbox's applied migrations")
        .into_iter()
        .map(|v| v.to_string())
        .collect();
    assert_eq!(
        applied, embedded,
        "the sandbox's migrations differ from this build's"
    );
}

/// Drops a throwaway database (and its clone) however the test ends.
struct Throwaway(Vec<String>);

impl Drop for Throwaway {
    fn drop(&mut self) {
        let Ok(mut admin) = PgConnection::establish(&common::admin_url()) else {
            return;
        };
        for name in &self.0 {
            let _ = diesel::sql_query(format!("ALTER DATABASE \"{name}\" IS_TEMPLATE FALSE"))
                .execute(&mut admin);
            let _ = diesel::sql_query(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                .execute(&mut admin);
        }
    }
}

/// Separate sessions racing for a template that doesn't exist yet: one
/// builds it, the rest wait and reuse it, and nothing stays connected to it.
#[test]
fn concurrent_first_use_builds_the_template_once() {
    const CALLERS: usize = 4;
    let suffix = &Uuid::new_v4().simple().to_string()[..16];
    let name = format!("nosdesk_tpl_race_{suffix}");
    let clone = format!("nosdesk_tpl_clone_{suffix}");
    let _cleanup = Throwaway(vec![clone.clone(), name.clone()]);

    let builds = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(CALLERS));
    let callers: Vec<_> = (0..CALLERS)
        .map(|_| {
            let (name, builds, start) = (name.clone(), builds.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                common::ensure_template(&name, |conn| {
                    builds.fetch_add(1, Ordering::SeqCst);
                    diesel::sql_query("CREATE TABLE built (id int)")
                        .execute(conn)
                        .expect("populate template");
                    // Long enough for every other caller to arrive mid-build.
                    thread::sleep(Duration::from_millis(300));
                })
            })
        })
        .collect();
    let results: Vec<_> = callers.into_iter().map(|caller| caller.join()).collect();

    assert!(
        results.iter().all(|r| r.is_ok()),
        "every concurrent caller gets the template"
    );
    assert_eq!(
        builds.load(Ordering::SeqCst),
        1,
        "the template is built once"
    );
    assert_eq!(
        results.iter().filter(|r| matches!(r, Ok(true))).count(),
        1,
        "exactly one caller reports building it"
    );

    let mut admin = PgConnection::establish(&common::admin_url()).expect("connect to admin DB");
    assert_eq!(is_template(&mut admin, &name), Some(true));
    diesel::sql_query(format!("CREATE DATABASE \"{clone}\" TEMPLATE \"{name}\""))
        .execute(&mut admin)
        .expect("clone the template straight away: nothing is left connected to it");
}

fn create_database(admin: &mut PgConnection, name: &str, template: bool) {
    diesel::sql_query(format!("CREATE DATABASE \"{name}\""))
        .execute(admin)
        .expect("create database");
    if template {
        diesel::sql_query(format!("ALTER DATABASE \"{name}\" IS_TEMPLATE TRUE"))
            .execute(admin)
            .expect("mark template");
    }
}

/// A stale-template pass drops a template nobody holds, keeps one another
/// process holds while it clones from it, and never marks a half-built one
/// finished when its DROP fails.
#[test]
fn a_template_in_use_survives_a_stale_pass() {
    let fake = || {
        let hash = &Uuid::new_v4().simple().to_string()[..16];
        (
            format!("nosdesk_test_template_{hash}"),
            u64::from_str_radix(hash, 16).expect("hex") as i64,
        )
    };
    let (held, held_key) = fake();
    let (unheld, _) = fake();
    let (half_built, _) = fake();
    let _cleanup = Throwaway(vec![held.clone(), unheld.clone(), half_built.clone()]);

    let mut admin = PgConnection::establish(&common::admin_url()).expect("connect to admin DB");
    create_database(&mut admin, &held, true);
    create_database(&mut admin, &unheld, true);
    create_database(&mut admin, &half_built, false);

    // Another process cloning from `held` holds its key shared.
    let mut other_process =
        PgConnection::establish(&common::admin_url()).expect("connect as another process");
    diesel::sql_query(format!("SELECT pg_advisory_lock_shared({held_key})"))
        .execute(&mut other_process)
        .expect("hold the template");
    // A stray session in a half-built template makes its DROP fail.
    let _stray = PgConnection::establish(&common::database_url(&half_built))
        .expect("connect to half-built DB");

    common::drop_stale_templates_now();

    let after = (
        is_template(&mut admin, &held),
        is_template(&mut admin, &unheld),
        is_template(&mut admin, &half_built),
    );
    assert_eq!(
        after,
        (Some(true), None, Some(false)),
        "(held, unheld, half-built): a held template is kept, an unheld one is dropped, \
         and a half-built one whose DROP failed stays unfinished"
    );
}
