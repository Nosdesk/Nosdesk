//! Integration tests clone their databases from a template built for this
//! build's migrations, and concurrent first use builds it once.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
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
/// the hash `build.rs` takes over `migrations/`.
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
