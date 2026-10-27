//! Failed-search gaps count the searches people settled on.
//!
//! - A person's search-as-you-type requests ("Sea", "Sear", "Searching
//!   worka", "Searching works") count as one search, for the query they
//!   ended on.
//! - One search logged twice (the old client sent each keystroke twice)
//!   counts once.
//! - Clearing the box and typing something else starts a new search.
//! - One-character queries and ticket numbers aren't demand for a doc.
//! - The next detection run dismisses open gaps raised from keystrokes, and
//!   leaves real ones, drafting ones and ones with nothing left to judge.
//! - A dismissed gap comes back only when new demand matches what was
//!   dismissed.
//! - The queue can dismiss several gaps at once and undo it.
//! - The search route logs each request once, with who ran it.

#![allow(clippy::expect_used)]

use chrono::{Duration, NaiveDateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use backend::db::DbConnection;
use backend::models::{
    DocumentationStatus, NewDocumentationPage, NewKnowledgeGap, NewKnowledgeGapSignal,
    NewSearchQueryLog,
};
use backend::repository::{knowledge_gaps as gaps, search_query_log};
use backend::schema::search_query_log as log_table;

/// Log a search `who` ran at `at`, as the search route would have.
fn logged(conn: &mut DbConnection, who: Option<Uuid>, raw: &str, hits: i32, at: NaiveDateTime) {
    diesel::insert_into(log_table::table)
        .values(&NewSearchQueryLog {
            query_raw: raw.to_string(),
            query_norm: search_query_log::normalise_query(raw),
            result_count: hits,
            searched_at: at,
            user_uuid: who,
        })
        .execute(conn)
        .expect("log row");
}

/// Log `keystrokes` as one person typing, half a second apart from `start`.
fn typed(conn: &mut DbConnection, who: Option<Uuid>, keystrokes: &[&str], start: NaiveDateTime) {
    for (i, raw) in keystrokes.iter().enumerate() {
        logged(
            conn,
            who,
            raw,
            0,
            start + Duration::milliseconds(500 * i as i64),
        );
    }
}

fn person(conn: &mut DbConnection, name: &str) -> Uuid {
    crate::common::insert_user(conn, name).uuid
}

/// A draft page by a new author, for a gap to be written in or resolved by.
fn doc_page(conn: &mut DbConnection, title: &str) -> i32 {
    use backend::schema::documentation_pages;
    let author = crate::common::insert_user(conn, "Writer").uuid;
    diesel::insert_into(documentation_pages::table)
        .values(&NewDocumentationPage {
            uuid: Uuid::now_v7(),
            title: title.to_string(),
            slug: format!(
                "{}-{}",
                title.to_lowercase(),
                &Uuid::new_v4().simple().to_string()[..8]
            ),
            icon: None,
            cover_image: None,
            status: DocumentationStatus::Draft,
            created_by: author,
            last_edited_by: author,
            parent_id: None,
            display_order: None,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: false,
        })
        .returning(documentation_pages::id)
        .get_result(conn)
        .expect("insert page")
}

fn ago(minutes: i64) -> NaiveDateTime {
    Utc::now().naive_utc() - Duration::minutes(minutes)
}

/// Every open or drafting gap's title.
fn open_titles(conn: &mut DbConnection) -> Vec<String> {
    let mut titles: Vec<String> = gaps::list_gaps(conn, gaps::GapListFilter::default())
        .expect("list")
        .into_iter()
        .map(|g| g.title)
        .collect();
    titles.sort();
    titles
}

/// A gap as the detector made them before searches were read as chains: one
/// `failed_search` signal for `query`.
fn old_search_gap(conn: &mut DbConnection, query: &str, count: i64) -> i64 {
    let gap = gaps::create_gap(
        conn,
        NewKnowledgeGap {
            title: format!("Searched: \"{query}\""),
            description: None,
            status: gaps::STATUS_OPEN.to_string(),
            created_by: None,
            impact_score: 0,
            evidence_count: 0,
            last_evidence_at: None,
        },
    )
    .expect("gap");
    gaps::attach_signal(
        conn,
        NewKnowledgeGapSignal {
            gap_id: gap.id,
            signal_type: gaps::SIGNAL_FAILED_SEARCH.to_string(),
            source_kind: gaps::SOURCE_SEARCH_QUERY.to_string(),
            source_ref: search_query_log::normalise_query(query),
            payload: serde_json::json!({ "query_sample": query, "count": count }),
            confidence: 40,
            detected_by: None,
        },
    )
    .expect("signal");
    gaps::recompute_aggregates(conn, gap.id).expect("aggregates");
    gap.id
}

#[test]
fn a_typing_chain_counts_as_the_query_it_ended_on() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let chain = [
        "Se",
        "Sea",
        "Sear",
        "Search",
        "Searchi",
        "Searching work",
        "Searching worka",
        "Searching work",
        "Searching works",
    ];
    for (i, name) in ["Ana", "Ben"].into_iter().enumerate() {
        let who = person(&mut conn, name);
        typed(&mut conn, Some(who), &chain, ago(60 * (i as i64 + 1)));
    }

    let found = search_query_log::aggregate_failed_searches(&mut conn, 30, 1).expect("aggregate");
    let queries: Vec<(&str, i64, i64)> = found
        .iter()
        .map(|a| (a.query_norm.as_str(), a.count, a.people))
        .collect();
    assert_eq!(queries, vec![("searching works", 2, 2)]);
    assert_eq!(found[0].query_sample, "Searching works");

    let stats = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(stats.gaps_created, 1);
    assert_eq!(
        open_titles(&mut conn),
        vec!["Searched: \"Searching works\""]
    );
}

#[test]
fn one_search_logged_twice_counts_once() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    let at = ago(30);
    // The old client sent every request twice, from two listeners: Ana's
    // search, and one from before searches carried a person.
    for who in [Some(ana), None] {
        logged(&mut conn, who, "Reset MFA", 0, at);
        logged(
            &mut conn,
            who,
            "Reset MFA",
            0,
            at + Duration::milliseconds(3),
        );
    }

    let found = search_query_log::aggregate_failed_searches(&mut conn, 30, 1).expect("aggregate");
    let counted: Vec<(i64, i64)> = found.iter().map(|a| (a.count, a.people)).collect();
    assert_eq!(counted, vec![(2, 2)], "two searches, not four");
}

#[test]
fn case_and_spacing_variants_are_one_query() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    let ben = person(&mut conn, "Ben");
    search_query_log::log_query(&mut conn, Some(ana), "VPN  setup", 0).expect("log");
    search_query_log::log_query(&mut conn, Some(ben), " vpn Setup ", 0).expect("log");

    let found = search_query_log::aggregate_failed_searches(&mut conn, 30, 2).expect("aggregate");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].query_norm, "vpn setup");
    assert_eq!(
        found[0].query_sample, "vpn Setup",
        "the latest form, trimmed"
    );
}

#[test]
fn clearing_the_box_and_typing_again_is_a_new_search() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    let ben = person(&mut conn, "Ben");
    for (who, start) in [(ana, ago(120)), (ben, ago(60))] {
        // "vpn" found nothing; cleared, then "visio".
        typed(
            &mut conn,
            Some(who),
            &["vp", "vpn", "v", "vi", "visio"],
            start,
        );
    }

    let mut found: Vec<String> = search_query_log::aggregate_failed_searches(&mut conn, 30, 2)
        .expect("aggregate")
        .into_iter()
        .map(|a| a.query_norm)
        .collect();
    found.sort();
    assert_eq!(found, vec!["visio", "vpn"]);
}

#[test]
fn a_search_refined_after_the_window_counts_on_its_own() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    let start = ago(60);
    logged(&mut conn, Some(ana), "printer", 0, start);
    logged(
        &mut conn,
        Some(ana),
        "printer driver",
        0,
        start + Duration::seconds(search_query_log::CHAIN_WINDOW_SECS + 5),
    );
    // Within the window, the same refinement replaces the first query.
    logged(
        &mut conn,
        Some(ana),
        "scanner",
        0,
        start + Duration::seconds(100),
    );
    logged(
        &mut conn,
        Some(ana),
        "scanner driver",
        0,
        start + Duration::seconds(104),
    );

    let mut found: Vec<String> = search_query_log::aggregate_failed_searches(&mut conn, 30, 1)
        .expect("aggregate")
        .into_iter()
        .map(|a| a.query_norm)
        .collect();
    found.sort();
    assert_eq!(found, vec!["printer", "printer driver", "scanner driver"]);
}

#[test]
fn a_search_that_found_a_doc_ends_the_chain_without_counting() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    let start = ago(60);
    logged(&mut conn, Some(ana), "print", 0, start);
    logged(
        &mut conn,
        Some(ana),
        "printer",
        3,
        start + Duration::seconds(1),
    );

    let found = search_query_log::aggregate_failed_searches(&mut conn, 30, 1).expect("aggregate");
    assert!(found.is_empty(), "\"print\" led to a doc: {found:?}");
}

#[test]
fn short_queries_and_ticket_numbers_are_not_demand() {
    assert!(!search_query_log::is_demand("t"));
    assert!(!search_query_log::is_demand("83"));
    assert!(!search_query_log::is_demand("#83"));
    assert!(!search_query_log::is_demand("#"));
    assert!(search_query_log::is_demand("ip"));
    assert!(search_query_log::is_demand("#vpn"));
    assert!(search_query_log::is_demand("2fa"));

    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ana = person(&mut conn, "Ana");
    for q in ["T", "  t ", "83", "#83"] {
        search_query_log::log_query(&mut conn, Some(ana), q, 0).expect("log");
    }
    let rows: i64 = log_table::table
        .count()
        .get_result(&mut conn)
        .expect("count");
    assert_eq!(rows, 0, "nothing logged");

    // Rows logged before the check are skipped by the detector.
    logged(&mut conn, None, "T", 0, ago(90));
    logged(&mut conn, None, "T", 0, ago(30));
    let found = search_query_log::aggregate_failed_searches(&mut conn, 30, 1).expect("aggregate");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn detection_retires_gaps_raised_from_keystrokes() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();

    // The owner's report: one person typed "Searching works" on a phone, the
    // old client logged every keystroke twice, and each prefix became a gap.
    let chain = [
        "T",
        "Sea",
        "Sear",
        "Search",
        "Searchi",
        "Searching work",
        "Searching worka",
        "Searching works",
    ];
    let keystrokes = [
        "T",
        "Sea",
        "Sear",
        "Search",
        "Searchi",
        "Searching work",
        "Searching worka",
        "Searching work",
        "Searching works",
    ];
    let start = ago(8 * 24 * 60);
    for (i, raw) in keystrokes.iter().enumerate() {
        let at = start + Duration::milliseconds(700 * i as i64);
        logged(&mut conn, None, raw, 0, at);
        logged(&mut conn, None, raw, 0, at + Duration::milliseconds(2));
    }
    let noise: Vec<i64> = chain
        .iter()
        .map(|q| old_search_gap(&mut conn, q, 2))
        .collect();

    // A real gap from the same era: two searches days apart, each sent twice.
    for at in [ago(9 * 24 * 60), ago(3 * 24 * 60)] {
        logged(&mut conn, None, "printer offline", 0, at);
        logged(
            &mut conn,
            None,
            "printer offline",
            0,
            at + Duration::milliseconds(2),
        );
    }
    let real = old_search_gap(&mut conn, "printer offline", 4);

    // Someone is writing a doc for another prefix; leave it to them.
    let at = ago(5 * 24 * 60);
    typed(&mut conn, None, &["Pri", "Pri", "Printer", "Printer"], at);
    let drafting = old_search_gap(&mut conn, "Pri", 2);
    let draft = doc_page(&mut conn, "Printing");
    gaps::start_drafting(&mut conn, drafting, draft).expect("draft");

    // A gap whose log rows are gone has nothing to judge it by.
    let unjudged = old_search_gap(&mut conn, "mailbox full", 2);

    let stats = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    let mut expected = noise.clone();
    expected.sort_unstable();
    assert_eq!(stats.retired_gap_ids, expected);
    assert_eq!(stats.gaps_created, 0);

    for id in &noise {
        let gap = gaps::get_gap(&mut conn, *id).expect("gap");
        assert_eq!(gap.status, gaps::STATUS_DISMISSED, "{}", gap.title);
        assert_eq!(gap.dismissed_by, None, "dismissed by detection");
    }
    for (id, status) in [
        (real, gaps::STATUS_OPEN),
        (drafting, gaps::STATUS_DRAFTING),
        (unjudged, gaps::STATUS_OPEN),
    ] {
        assert_eq!(gaps::get_gap(&mut conn, id).expect("gap").status, status);
    }

    // The next run has nothing more to do, and brings none back.
    let again = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!((again.gaps_created, again.gaps_retired), (0, 0));
}

#[test]
fn a_dismissed_gap_comes_back_only_for_as_much_new_demand() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let admin = person(&mut conn, "Admin");
    let people: Vec<Uuid> = (0..6)
        .map(|i| person(&mut conn, &format!("Searcher {i}")))
        .collect();
    for who in &people[..3] {
        search_query_log::log_query(&mut conn, Some(*who), "badge printer", 0).expect("log");
    }
    let first = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    let gap = gaps::get_gap(&mut conn, first.new_gap_ids[0]).expect("gap");
    assert_eq!(gap.impact_score, 3);
    gaps::dismiss_gap(&mut conn, gap.id, admin).expect("dismiss");

    // Two new searches would raise a new gap, but three were dismissed.
    for who in &people[3..5] {
        search_query_log::log_query(&mut conn, Some(*who), "Badge printer", 0).expect("log");
    }
    let again = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(again.gaps_created, 0);
    assert!(open_titles(&mut conn).is_empty());

    search_query_log::log_query(&mut conn, Some(people[5]), "badge printer", 0).expect("log");
    let later = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(
        later.gaps_created, 1,
        "three new searches match the three dismissed"
    );
}

#[test]
fn a_resolved_gap_comes_back_for_the_usual_demand() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = person(&mut conn, "Admin");
    let people: Vec<Uuid> = (0..5)
        .map(|i| person(&mut conn, &format!("Searcher {i}")))
        .collect();
    for who in &people[..3] {
        search_query_log::log_query(&mut conn, Some(*who), "guest wifi", 0).expect("log");
    }
    let first = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    let page = doc_page(&mut conn, "Guest wifi");
    gaps::resolve_gap(&mut conn, first.new_gap_ids[0], page, Some(author)).expect("resolve");

    // The doc doesn't answer these words: two more failures reopen it.
    for who in &people[3..] {
        search_query_log::log_query(&mut conn, Some(*who), "guest wifi", 0).expect("log");
    }
    let later = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(later.gaps_created, 1);
}

#[test]
fn several_gaps_are_dismissed_at_once_and_the_dismissal_undone() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let admin = person(&mut conn, "Admin");
    let open = old_search_gap(&mut conn, "keyboard", 2);
    let drafting = old_search_gap(&mut conn, "monitor", 2);
    let draft = doc_page(&mut conn, "Monitors");
    gaps::start_drafting(&mut conn, drafting, draft).expect("draft");
    let closed = old_search_gap(&mut conn, "mouse", 2);
    gaps::dismiss_gap(&mut conn, closed, admin).expect("dismiss");

    let dismissed: Vec<i64> = gaps::dismiss_gaps(&mut conn, &[open, drafting, closed], admin)
        .expect("dismiss")
        .into_iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(dismissed, vec![open, drafting], "the closed one is skipped");
    assert!(open_titles(&mut conn).is_empty());

    let reopened: Vec<i64> = gaps::reopen_gaps(&mut conn, &[open, drafting])
        .expect("reopen")
        .into_iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(reopened, vec![open, drafting]);
    let open_gap = gaps::get_gap(&mut conn, open).expect("gap");
    assert_eq!(open_gap.status, gaps::STATUS_OPEN);
    assert_eq!((open_gap.dismissed_at, open_gap.dismissed_by), (None, None));
    assert_eq!(
        gaps::get_gap(&mut conn, drafting).expect("gap").status,
        gaps::STATUS_DRAFTING,
        "back to its draft"
    );
}

mod search_route {
    use std::sync::Arc;

    use actix_web::dev::Service;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use diesel::prelude::*;
    use uuid::Uuid;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::schema::search_query_log as log_table;
    use backend::services::search::SearchService;
    use backend::sync::actor::ActorContext;

    use crate::common;

    fn claims(user: Uuid) -> Claims {
        Claims {
            sub: user.to_string(),
            name: "Searcher".to_string(),
            email: "searcher@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        }
    }

    #[actix_web::test]
    async fn each_search_is_logged_once_with_who_ran_it() {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
        let ws = seeded.a.workspace_id;
        let agent = seeded.a.admin_uuid;
        let workspace = WorkspaceContext {
            workspace_id: ws,
            workspace_uuid: seeded.a.workspace_uuid,
            slug: seeded.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let search_dir = tempfile::tempdir().expect("tempdir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        let corr = Uuid::now_v7();
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(search.clone()))
                .wrap_fn(move |req, srv| {
                    let actor = ActorContext::user(agent, Some(corr)).with_workspace(ws);
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims(agent));
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::search::config)),
        )
        .await;

        for uri in [
            "/api/search?q=Quokka%20policy&limit=50",
            // Not documentation: no demand for a doc.
            "/api/search?q=quokka%20laptop&types=device",
            // A one-character start of a search.
            "/api/search?q=q",
        ] {
            let resp =
                http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request())
                    .await;
            assert!(resp.status().is_success(), "{uri}: {}", resp.status());
        }

        // The log write runs after the response, off the request path.
        let mut rows: Vec<(String, Option<Uuid>, i32)> = Vec::new();
        for _ in 0..100 {
            let mut conn = pool.get().expect("conn");
            rows = log_table::table
                .filter(log_table::workspace_id.eq(ws))
                .select((
                    log_table::query_norm,
                    log_table::user_uuid,
                    log_table::result_count,
                ))
                .load(&mut conn)
                .expect("log rows");
            if !rows.is_empty() {
                break;
            }
            actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        // Give a stray second write the chance to land before counting.
        actix_web::rt::time::sleep(std::time::Duration::from_millis(200)).await;
        let mut conn = pool.get().expect("conn");
        let total: i64 = log_table::table
            .filter(log_table::workspace_id.eq(ws))
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(rows, vec![("quokka policy".to_string(), Some(agent), 0)]);
        assert_eq!(total, 1);
    }
}
