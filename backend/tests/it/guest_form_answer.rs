//! The request form answers every address the same way, so it can't be asked
//! whether an address has an account here or anywhere on the deployment.
//!
//! With confirmation on or off, a new address, one whose unverified account is
//! reused, and one with a verified account (whose submission is set aside for
//! a sign-in) get one status and one body. Nothing ticket-derived is in it:
//! no ticket id or number, no status link, no word on whether mail went out.
//! The body says what to do if you already have an account.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::http::StatusCode;
use diesel::prelude::*;

use backend::handlers::guest::{file_guest_submission, GuestSubmission};
use backend::models::TicketPriority;
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::services::search::SearchService;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestDb, TestPool};
use crate::portal_session::context_of;

const REF: &str = "test:guest_form_answer";

struct Form {
    _db: TestDb,
    pool: TestPool,
    a: i32,
    b: i32,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    search: Arc<SearchService>,
    storage: Arc<dyn Storage>,
}

fn form() -> Form {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("search"));
    let storage_dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: storage_dir.path().to_string_lossy().into_owned(),
    });
    Form {
        a: seeded.a.workspace_id,
        b: seeded.b.workspace_id,
        _db: db,
        pool,
        _dirs: (search_dir, storage_dir),
        search,
        storage,
    }
}

impl Form {
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

    /// An account with `email` as its verified primary address.
    fn verified_account(&self, email: &str) {
        let mut c = self._db.pool_with_size(1).get().expect("conn");
        let user = common::insert_plain_user(&mut c, "Verified Person");
        diesel::insert_into(backend::schema::user_emails::table)
            .values(&backend::models::NewUserEmail {
                user_uuid: user,
                email: email.into(),
                email_type: "personal".into(),
                is_primary: true,
                is_verified: true,
                source: None,
            })
            .execute(&mut c)
            .expect("email");
    }

    /// Tickets B holds for a requester with `email`.
    fn tickets_for(&self, email: &str) -> i64 {
        use backend::schema::{tickets, user_emails};
        let mut c = self._db.pool_with_size(1).get().expect("conn");
        tickets::table
            .inner_join(
                user_emails::table.on(user_emails::user_uuid
                    .nullable()
                    .eq(tickets::requester_uuid)),
            )
            .filter(tickets::workspace_id.eq(self.b))
            .filter(user_emails::email.eq(email))
            .count()
            .get_result(&mut c)
            .expect("count")
    }

    /// Submit B's form from `email` and read the answer.
    async fn submit(
        &self,
        email: &str,
        verification_required: bool,
    ) -> (StatusCode, serde_json::Value) {
        let ctx = context_of(&mut self.pool.get().expect("conn"), self.b);
        let req = actix_web::test::TestRequest::default().to_http_request();
        let submission = GuestSubmission {
            email,
            name: "Sam",
            title: "Printer jammed",
            description: "It jams on every page.",
            priority: TicketPriority::Medium,
            category_id: None,
            verification_required,
            lookup_token: uuid::Uuid::new_v4(),
        };
        let resp = file_guest_submission(
            &self.pool,
            &self.search,
            &self.storage,
            &req,
            &ctx,
            &submission,
            &[],
        )
        .await
        .expect("answered");
        let status = resp.status();
        let bytes = actix_web::body::to_bytes(resp.into_body())
            .await
            .expect("body");
        (status, serde_json::from_slice(&bytes).expect("json"))
    }
}

async fn one_answer_for_every_address(verification_required: bool) {
    let f = form();
    let tag = if verification_required { "on" } else { "off" };
    let fresh = format!("new-{tag}@guest.example");
    let reused = format!("reused-{tag}@guest.example");
    let claimed = format!("claimed-{tag}@guest.example");
    f.guest_of_a(&reused);
    f.verified_account(&claimed);

    let answers = [
        f.submit(&fresh, verification_required).await,
        f.submit(&reused, verification_required).await,
        f.submit(&claimed, verification_required).await,
    ];
    for answer in &answers {
        assert_eq!(answer, &answers[0], "the same answer for every address");
    }
    let (status, body) = &answers[0];
    assert_eq!(*status, StatusCode::ACCEPTED);
    let keys: Vec<&str> = body
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["message", "verification_required"], "{body}");
    assert_eq!(body["verification_required"], verification_required);
    let expected = if verification_required {
        "Check your email for a link to confirm your request."
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

    // The answers match while the outcomes don't: the claimed address filed
    // nothing, the others filed their request.
    assert_eq!(f.tickets_for(&fresh), 1);
    assert_eq!(f.tickets_for(&reused), 1);
    assert_eq!(f.tickets_for(&claimed), 0);
}

#[actix_web::test]
async fn with_confirmation_off_every_address_gets_one_answer() {
    one_answer_for_every_address(false).await;
}

#[actix_web::test]
async fn with_confirmation_on_every_address_gets_one_answer() {
    one_answer_for_every_address(true).await;
}
