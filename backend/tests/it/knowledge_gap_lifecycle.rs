//! A knowledge gap moves open -> drafting -> resolved as its doc is written,
//! and the hourly detector doesn't bring back a gap someone closed.
//!
//! - "Write this doc" ties a draft page to the gap (drafting); publishing the
//!   page resolves the gap and links the page to the gap's tickets; deleting
//!   the draft sends the gap back to open.
//! - Linking a page as resolving a flagged ticket moves that ticket's gap on
//!   (published: resolved, draft: drafting). A cluster gap is left alone.
//! - A ticket a live page already resolves can't be flagged.
//! - Detection doesn't recreate a dismissed gap from the same evidence, only
//!   from evidence that arrived after the dismissal.

#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::models::{
    DocumentationPage, DocumentationPageUpdate, DocumentationStatus, NewDocumentationPage,
    NewKnowledgeGap, NewKnowledgeGapSignal, NewTicket, User,
};
use backend::repository::{
    documentation, documentation_page_tickets as links, knowledge_gaps as gaps, search_query_log,
};

const WS: i32 = 1;

fn ticket(conn: &mut DbConnection, title: &str) -> i32 {
    use backend::schema::{tickets, workflow_states};
    let state: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(WS))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(conn)
        .expect("default workflow state");
    diesel::insert_into(tickets::table)
        .values(&NewTicket {
            title: title.to_string(),
            workflow_state_id: state,
            ..Default::default()
        })
        .returning(tickets::id)
        .get_result(conn)
        .expect("insert ticket")
}

fn page(
    conn: &mut DbConnection,
    author: &User,
    slug: &str,
    status: DocumentationStatus,
) -> DocumentationPage {
    use backend::schema::documentation_pages;
    diesel::insert_into(documentation_pages::table)
        .values(&NewDocumentationPage {
            uuid: uuid::Uuid::now_v7(),
            title: format!("Doc {slug}"),
            slug: slug.to_string(),
            icon: None,
            cover_image: None,
            status,
            created_by: author.uuid,
            last_edited_by: author.uuid,
            parent_id: None,
            display_order: None,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: false,
        })
        .get_result(conn)
        .expect("insert page")
}

fn set_status(conn: &mut DbConnection, page_id: i32, status: DocumentationStatus) {
    documentation::update_documentation_page(
        conn,
        page_id,
        &DocumentationPageUpdate {
            status: Some(status),
            ..Default::default()
        },
    )
    .expect("update page status");
}

fn resolves_link_exists(conn: &mut DbConnection, page_id: i32, ticket_id: i32) -> bool {
    links::links_for_ticket(conn, ticket_id)
        .expect("links")
        .iter()
        .any(|l| l.page_id == page_id && l.link_type == links::LINK_RESOLVES)
}

#[test]
fn writing_a_gaps_doc_drafts_it_and_publishing_resolves_it() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Writer");
    let t = ticket(&mut conn, "VPN drops");
    let (gap, _, _) =
        gaps::flag_ticket(&mut conn, t, "VPN drops", author.uuid, None).expect("flag");

    let draft = page(&mut conn, &author, "vpn-drops", DocumentationStatus::Draft);
    let drafting = gaps::write_gap_as_page(&mut conn, gap.id, draft.id, false, Some(author.uuid))
        .expect("write")
        .expect("gap moved");
    assert_eq!(drafting.status, gaps::STATUS_DRAFTING);
    assert_eq!(drafting.draft_page_id, Some(draft.id));

    set_status(&mut conn, draft.id, DocumentationStatus::Published);
    let resolved = gaps::get_gap(&mut conn, gap.id).expect("gap");
    assert_eq!(resolved.status, gaps::STATUS_RESOLVED);
    assert_eq!(resolved.resolved_page_id, Some(draft.id));
    assert_eq!(resolved.draft_page_id, None);
    assert!(
        resolves_link_exists(&mut conn, draft.id, t),
        "page linked to the gap's ticket"
    );
}

#[test]
fn deleting_the_draft_sends_the_gap_back_to_open() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Writer");
    let t = ticket(&mut conn, "Printer jams");
    let (gap, _, _) =
        gaps::flag_ticket(&mut conn, t, "Printer jams", author.uuid, None).expect("flag");
    let draft = page(
        &mut conn,
        &author,
        "printer-jams",
        DocumentationStatus::Draft,
    );
    gaps::write_gap_as_page(&mut conn, gap.id, draft.id, false, None).expect("write");

    set_status(&mut conn, draft.id, DocumentationStatus::Deleted);
    let reopened = gaps::get_gap(&mut conn, gap.id).expect("gap");
    assert_eq!(reopened.status, gaps::STATUS_OPEN);
    assert_eq!(reopened.draft_page_id, None);
}

#[test]
fn linking_a_page_as_resolving_a_flagged_ticket_moves_its_gap_on() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Writer");

    // Published page: the ticket's gap resolves at once.
    let t1 = ticket(&mut conn, "Wifi drops");
    let (g1, _, _) =
        gaps::flag_ticket(&mut conn, t1, "Wifi drops", author.uuid, None).expect("flag");
    let published = page(&mut conn, &author, "wifi", DocumentationStatus::Published);
    gaps::link_page_resolves_ticket(&mut conn, published.id, t1, Some(author.uuid)).expect("link");
    assert_eq!(
        gaps::get_gap(&mut conn, g1.id).expect("gap").status,
        gaps::STATUS_RESOLVED
    );

    // Draft page: the gap starts drafting on it.
    let t2 = ticket(&mut conn, "Badge reader");
    let (g2, _, _) =
        gaps::flag_ticket(&mut conn, t2, "Badge reader", author.uuid, None).expect("flag");
    let draft = page(&mut conn, &author, "badge", DocumentationStatus::Draft);
    gaps::link_page_resolves_ticket(&mut conn, draft.id, t2, None).expect("link");
    let g2 = gaps::get_gap(&mut conn, g2.id).expect("gap");
    assert_eq!(g2.status, gaps::STATUS_DRAFTING);
    assert_eq!(g2.draft_page_id, Some(draft.id));
}

#[test]
fn a_cluster_gap_is_not_resolved_by_one_tickets_doc() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Writer");
    let t = ticket(&mut conn, "Laptop battery");
    let cluster = gaps::create_gap(
        &mut conn,
        NewKnowledgeGap {
            title: "Battery cluster".into(),
            description: None,
            status: gaps::STATUS_OPEN.into(),
            created_by: None,
            impact_score: 0,
            evidence_count: 0,
            last_evidence_at: None,
        },
    )
    .expect("gap");
    gaps::attach_signal(
        &mut conn,
        NewKnowledgeGapSignal {
            gap_id: cluster.id,
            signal_type: gaps::SIGNAL_TICKET_CLUSTER.into(),
            source_kind: gaps::SOURCE_CLUSTER_KEY.into(),
            source_ref: "battery".into(),
            payload: serde_json::json!({ "ticket_ids": [t] }),
            confidence: 50,
            detected_by: None,
        },
    )
    .expect("signal");
    let published = page(
        &mut conn,
        &author,
        "battery",
        DocumentationStatus::Published,
    );
    gaps::link_page_resolves_ticket(&mut conn, published.id, t, None).expect("link");
    assert_eq!(
        gaps::get_gap(&mut conn, cluster.id).expect("gap").status,
        gaps::STATUS_OPEN
    );
}

#[test]
fn a_ticket_with_a_live_resolving_page_reports_it() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Writer");
    let t = ticket(&mut conn, "Monitor flicker");
    assert!(links::resolving_page_for_ticket(&mut conn, t)
        .expect("read")
        .is_none());

    let doc = page(
        &mut conn,
        &author,
        "flicker",
        DocumentationStatus::Published,
    );
    links::upsert_link(&mut conn, doc.id, t, links::LINK_RESOLVES, None).expect("link");
    let (id, _, slug) = links::resolving_page_for_ticket(&mut conn, t)
        .expect("read")
        .expect("documented");
    assert_eq!((id, slug.as_str()), (doc.id, "flicker"));

    // A deleted page doesn't count.
    set_status(&mut conn, doc.id, DocumentationStatus::Deleted);
    assert!(links::resolving_page_for_ticket(&mut conn, t)
        .expect("read")
        .is_none());
}

#[test]
fn detection_does_not_recreate_a_dismissed_gap_without_new_evidence() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let author = crate::common::insert_user(&mut conn, "Admin");
    for _ in 0..2 {
        search_query_log::log_query(&mut conn, "reset mfa", 0).expect("log");
    }

    let first = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(first.gaps_created, 1);
    gaps::dismiss_gap(&mut conn, first.new_gap_ids[0], author.uuid).expect("dismiss");

    // The next hourly run sees the same two searches: nothing new.
    let again = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(again.gaps_created, 0);

    // Two more failed searches after the dismissal: that's new demand.
    for _ in 0..2 {
        search_query_log::log_query(&mut conn, "Reset MFA", 0).expect("log");
    }
    let later = gaps::run_failed_search_detection(&mut conn, None, 30, 2).expect("detect");
    assert_eq!(later.gaps_created, 1);
}

#[actix_web::test]
async fn the_hourly_job_detects_gaps_in_each_workspace() {
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(3);
    {
        let mut conn = pool.get().expect("conn");
        for _ in 0..2 {
            search_query_log::log_query(&mut conn, "printer offline", 0).expect("log");
        }
    }

    backend::services::scheduled_jobs::knowledge_gap_detection(pool.clone())
        .await
        .expect("job");

    let mut conn = pool.get().expect("conn");
    let open = gaps::list_gaps(
        &mut conn,
        gaps::GapListFilter {
            statuses: Vec::new(),
            limit: 50,
            offset: 0,
        },
    )
    .expect("list");
    assert!(
        open.iter().any(|g| g.title.contains("printer offline")),
        "the job found the failed search: {:?}",
        open.iter().map(|g| &g.title).collect::<Vec<_>>()
    );
}
