//! The request form answers every address the same way, so it can't be asked
//! whether an address has an account here or anywhere on the deployment.
//!
//! Through the form's own route, with confirmation on or off: a new address,
//! one whose unverified account is reused, one with a verified account, one
//! held as a verified secondary in another case, and (with confirmation on)
//! one with too many requests waiting all get one status, one set of headers
//! and one body. Nothing ticket-derived is in it, and the body says what to do
//! if you already have an account. What became of each submission is decided
//! after the answer goes out, so the answer's timing doesn't depend on it.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::{web, App};
use diesel::prelude::*;

use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::services::search::SearchService;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestDb, TestPool};
use crate::portal_session::context_of;

const REF: &str = "test:guest_form_answer";

struct Form {
    db: TestDb,
    pool: TestPool,
    a: i32,
    b: i32,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    search: Arc<SearchService>,
    storage: Arc<dyn Storage>,
}

/// Two workspaces; B's request form is on, with confirmation as given.
fn form(verification_required: bool) -> Form {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let b = seeded.b.workspace_id;
    clear_daily_count(b);
    let pool = db.runtime_pool(6);
    // A new workspace's settings row is made on first read.
    run_in_workspace(&pool, REF, b, |c| {
        backend::repository::site_settings::get_site_settings(c).map(|_| ())
    })
    .expect("settings row");
    {
        use backend::schema::site_settings as ss;
        let rows = diesel::update(ss::table.filter(ss::workspace_id.eq(b)))
            .set((
                ss::guest_tickets_enabled.eq(true),
                ss::guest_ticket_email_verification.eq(verification_required),
            ))
            .execute(&mut db.pool_with_size(1).get().expect("conn"))
            .expect("turn the form on");
        assert_eq!(rows, 1);
    }
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("search"));
    let storage_dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: storage_dir.path().to_string_lossy().into_owned(),
    });
    Form {
        a: seeded.a.workspace_id,
        b,
        db,
        pool,
        _dirs: (search_dir, storage_dir),
        search,
        storage,
    }
}

/// Forget the form's daily count for `ws` in the limiter's Redis, which
/// outlives the sandbox database, so earlier runs that reused the workspace
/// id can't spend this one's allowance. Redis being away is fine: the limit
/// fails open.
fn clear_daily_count(ws: i32) {
    let Ok(client) = redis::Client::open(backend::utils::rate_limit::get_redis_url()) else {
        return;
    };
    if let Ok(mut conn) = client.get_connection_with_timeout(std::time::Duration::from_secs(1)) {
        let _: redis::RedisResult<()> = redis::cmd("DEL")
            .arg(format!("guest_tickets_day:{ws}"))
            .query(&mut conn);
    }
}

/// The form's challenge for `ws`, solved, as a browser sends it. Issued ten
/// seconds ago, so it is past the minimum age.
fn solved_challenge(ws: i32) -> serde_json::Value {
    use ring::hmac;
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
    let outer = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let key = hmac::Key::new(
        hmac::HMAC_SHA256,
        hmac::sign(&outer, b"nosdesk-form-challenge-v1").as_ref(),
    );
    let now = chrono::Utc::now().timestamp();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let c = backend::utils::form_challenge::issue_with(&key, ws, now - 10, 7, &nonce);
    serde_json::json!({
        "challenge": c.challenge,
        "salt": c.salt,
        "signature": c.signature,
        "number": 7,
    })
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

impl Form {
    fn superuser(&self) -> backend::db::DbConnection {
        self.db.pool_with_size(1).get().expect("conn")
    }

    /// A guest account of workspace A, which B's form reuses.
    fn guest_of_a(&self, email: &str) {
        run_in_workspace(
            &self.pool,
            REF,
            self.a,
            |c| match find_or_create_guest_user(email, "Sam", c, None)? {
                GuestUserResult::Created(_) => Ok(()),
                _ => panic!("expected a new guest"),
            },
        )
        .expect("guest");
    }

    /// A member of B with these addresses: (address, primary, verified).
    fn member_of_b(&self, addresses: &[(&str, bool, bool)]) -> uuid::Uuid {
        let mut c = self.superuser();
        let user = common::insert_plain_user(&mut c, "Account Holder");
        for (email, is_primary, is_verified) in addresses {
            diesel::insert_into(backend::schema::user_emails::table)
                .values(&backend::models::NewUserEmail {
                    user_uuid: user,
                    email: (*email).into(),
                    email_type: "personal".into(),
                    is_primary: *is_primary,
                    is_verified: *is_verified,
                    source: None,
                })
                .execute(&mut c)
                .expect("email");
        }
        common::seat(&mut c, self.b, user, "member");
        user
    }

    /// Tickets B holds for a requester with `email`, in any case.
    fn tickets_for(&self, email: &str) -> i64 {
        diesel::sql_query(
            "SELECT count(*) AS n FROM tickets t JOIN user_emails e \
             ON e.user_uuid = t.requester_uuid \
             WHERE t.workspace_id = $1 AND lower(e.email) = lower($2)",
        )
        .bind::<diesel::sql_types::Integer, _>(self.b)
        .bind::<diesel::sql_types::Text, _>(email)
        .get_result::<Count>(&mut self.superuser())
        .expect("count")
        .n
    }

    /// The addresses `user`'s portal sign-in links were sent to.
    fn sign_in_links_to(&self, user: uuid::Uuid) -> Vec<String> {
        use backend::schema::reset_tokens as rt;
        rt::table
            .filter(rt::user_uuid.eq(user))
            .filter(rt::token_type.eq("portal_magic_link"))
            .select(rt::metadata)
            .load::<Option<serde_json::Value>>(&mut self.superuser())
            .expect("tokens")
            .into_iter()
            .filter_map(|m| m?.get("sent_to")?.as_str().map(str::to_string))
            .collect()
    }

    /// Confirmations queued to `recipient`, compared case-insensitively.
    fn confirmations_to(&self, recipient: &str) -> i64 {
        diesel::sql_query(
            "SELECT count(*) AS n FROM outbound_emails \
             WHERE lower(recipient) = lower($1) AND idempotency_key LIKE $2",
        )
        .bind::<diesel::sql_types::Text, _>(recipient)
        .bind::<diesel::sql_types::Text, _>(format!(
            "{}%",
            backend::services::transactional_email::GUEST_CONFIRMATION_KEY_PREFIX
        ))
        .get_result::<Count>(&mut self.superuser())
        .expect("count")
        .n
    }

    /// Mail queued to `recipient`, compared case-insensitively.
    fn mail_to(&self, recipient: &str) -> i64 {
        diesel::sql_query(
            "SELECT count(*) AS n FROM outbound_emails WHERE lower(recipient) = lower($1)",
        )
        .bind::<diesel::sql_types::Text, _>(recipient)
        .get_result::<Count>(&mut self.superuser())
        .expect("count")
        .n
    }

    /// Submit B's form from `email` through its route, and read the answer:
    /// status, headers (but the date) and body.
    async fn submit(&self, email: &str) -> (u16, Vec<(String, String)>, serde_json::Value) {
        // A canonical origin, so confirmation links can be built (self-hosted
        // would use FRONTEND_URL, which this binary leaves unset).
        self.submit_on(email, Some(format!("help-{}.example.com", self.b)))
            .await
    }

    /// [`Self::submit`] on an origin with `custom_domain` as its canonical
    /// host; with none, no confirmation link can be built.
    async fn submit_on(
        &self,
        email: &str,
        custom_domain: Option<String>,
    ) -> (u16, Vec<(String, String)>, serde_json::Value) {
        let mut ctx = context_of(&mut self.pool.get().expect("conn"), self.b);
        ctx.custom_domain = custom_domain;
        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .app_data(web::Data::new(self.storage.clone()))
                .app_data(web::Data::new(ctx))
                .wrap(actix_web::middleware::from_fn(resolve_workspace))
                .service(web::scope("/api/public").configure(backend::handlers::guest::config)),
        )
        .await;
        let req = actix_web::test::TestRequest::post()
            .uri("/api/public/tickets")
            .set_json(serde_json::json!({
                "name": "Sam",
                "email": email,
                "title": "Printer jammed",
                "description": "It jams on every page.",
                "challenge": solved_challenge(self.b),
            }))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        let status = resp.status().as_u16();
        let mut headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter(|(k, _)| k.as_str() != "date")
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        headers.sort();
        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        (status, headers, body)
    }

    /// Wait for the filings the answers left running, until `done` holds.
    async fn until(&self, what: &str, done: impl Fn() -> bool) {
        for _ in 0..200 {
            if done() {
                return;
            }
            actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {what}");
    }
}

/// Stands in for the workspace-context middleware: every request is for the
/// workspace the app was given.
async fn resolve_workspace(
    req: actix_web::dev::ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody>,
) -> Result<actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>, actix_web::Error> {
    use actix_web::HttpMessage as _;
    if let Some(ctx) = req.app_data::<web::Data<backend::extractors::WorkspaceContext>>() {
        let ctx = ctx.get_ref().clone();
        req.extensions_mut().insert(ctx);
    }
    next.call(req).await
}

async fn one_answer_for_every_address(verification_required: bool) {
    let f = form(verification_required);
    let tag = if verification_required { "on" } else { "off" };
    let fresh = format!("new-{tag}@example.com");
    let reused = format!("reused-{tag}@example.com");
    let claimed = format!("claimed-{tag}@example.com");
    f.guest_of_a(&reused);
    let holder = f.member_of_b(&[(&claimed, true, true)]);
    let alias_holder = f.member_of_b(&[
        (&format!("primary-{tag}@example.com"), true, true),
        (&format!("Alias-{tag}@Example.com"), false, true),
    ]);
    let alias_typed = format!("ALIAS-{tag}@example.COM");

    let mut answers = vec![
        f.submit(&fresh).await,
        f.submit(&reused).await,
        f.submit(&claimed).await,
        f.submit(&alias_typed).await,
    ];
    if verification_required {
        // An address with three requests waiting is told the same about a
        // fourth, which is not filed.
        let busy = format!("busy-{tag}@example.com");
        for n in 1..=3 {
            answers.push(f.submit(&busy).await);
            f.until("a held request", || f.tickets_for(&busy) == n)
                .await;
        }
        answers.push(f.submit(&busy).await);
        actix_web::rt::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(f.tickets_for(&busy), 3, "the fourth is not filed");
    }
    for answer in &answers {
        assert_eq!(answer, &answers[0], "the same answer for every address");
    }
    let (status, _, body) = &answers[0];
    assert_eq!(*status, 202);
    let keys: Vec<&str> = body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["message", "verification_required"], "{body}");
    assert_eq!(body["verification_required"], verification_required);
    let expected = if verification_required {
        "Check your email for what to do next."
    } else {
        "We'll be in touch by email."
    };
    assert_eq!(
        body["message"],
        format!(
            "{expected} If you already have an account with us, sign in and send your request \
             from there."
        )
    );

    // The answers match while the outcomes don't.
    f.until("the new address's request", || f.tickets_for(&fresh) == 1)
        .await;
    f.until("the reused address's request", || {
        f.tickets_for(&reused) == 1
    })
    .await;
    // Filed with the ticket: a confirmation for a new address always, and for
    // a reused one when its request is held.
    assert_eq!(f.confirmations_to(&fresh), 1);
    assert_eq!(
        f.confirmations_to(&reused),
        i64::from(verification_required)
    );
    f.until("the account holder's sign-in link", || {
        !f.sign_in_links_to(holder).is_empty()
    })
    .await;
    assert_eq!(
        f.tickets_for(&claimed),
        0,
        "an account holder files nothing"
    );
    // The verified secondary is that account's, in any case: the link goes
    // to the address typed, and nothing is filed.
    f.until("the alias holder's sign-in link", || {
        !f.sign_in_links_to(alias_holder).is_empty()
    })
    .await;
    assert_eq!(
        f.sign_in_links_to(alias_holder),
        [alias_typed.to_lowercase()]
    );
    f.until("the alias holder's sign-in mail", || {
        f.mail_to(&alias_typed) == 1
    })
    .await;
    assert_eq!(f.tickets_for(&alias_typed), 0);
}

/// A submission that fails to file is answered as any other: the failure is
/// the server's to log, not the submitter's to read.
#[actix_web::test]
async fn a_filing_that_fails_is_answered_the_same() {
    let f = form(false);
    let (status, _, body) = f.submit("before@example.com").await;
    f.until("a request", || f.tickets_for("before@example.com") == 1)
        .await;

    let mut c = f.superuser();
    diesel::sql_query(
        "CREATE FUNCTION test_refuse_ticket() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'ticket refused'; END $$",
    )
    .execute(&mut c)
    .expect("install fault");
    diesel::sql_query(
        "CREATE TRIGGER test_refuse_ticket BEFORE INSERT ON tickets FOR EACH ROW \
         EXECUTE FUNCTION test_refuse_ticket()",
    )
    .execute(&mut c)
    .expect("install fault");

    let (failed_status, _, failed_body) = f.submit("refused@example.com").await;
    assert_eq!((failed_status, &failed_body), (status, &body));
    actix_web::rt::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(f.tickets_for("refused@example.com"), 0);
}

#[actix_web::test]
async fn with_confirmation_off_every_address_gets_one_answer() {
    one_answer_for_every_address(false).await;
}

#[actix_web::test]
async fn with_confirmation_on_every_address_gets_one_answer() {
    one_answer_for_every_address(true).await;
}

/// A held request whose confirmation can't be queued (here, no origin to
/// build its link on) is not filed either, so nothing waits on a link nobody
/// was sent. It is answered as any other.
#[actix_web::test]
async fn a_held_request_without_its_confirmation_is_not_filed() {
    let f = form(true);
    let (status, _, body) = f.submit("linked@example.com").await;
    f.until("a held request", || {
        f.tickets_for("linked@example.com") == 1
    })
    .await;
    let answer = f.submit_on("unlinked@example.com", None).await;
    assert_eq!((answer.0, &answer.2), (status, &body));
    actix_web::rt::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(f.tickets_for("unlinked@example.com"), 0);
    assert_eq!(f.confirmations_to("unlinked@example.com"), 0);
}
