//! Integration tests for the per-document ownership layer that drives
//! multi-instance collaborative-editing routing (Phase 2 affinity). See
//! `docs/realtime-collab-affinity-design.md`.
//!
//! These exercise `CollabOwnership` against a real Redis (the dev-compose
//! instance, published on host loopback by `compose.dev.yaml`; URL preset
//! as `TEST_REDIS_URL` in `backend/.cargo/config.toml`, isolated on db
//! 15). They are the deterministic substitute for a fly multi-machine
//! deploy: the only thing they can't cover is fly-proxy's `fly-replay`
//! hop itself, which is fly infrastructure, not our logic.
//!
//! Each test uses a unique doc id so they can run in parallel without
//! key collisions.

#![allow(clippy::expect_used)] // tests fail loudly on purpose

use std::sync::Arc;

use backend::services::collab_ownership::CollabOwnership;

fn redis_url() -> String {
    std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL not set. Bring up the dev stack \
         (docker compose -f compose.yaml -f compose.dev.yaml up -d redis); \
         it publishes redis on 127.0.0.1:63799 and the URL is preset in \
         backend/.cargo/config.toml.",
    )
}

/// Fail loudly + clearly if Redis isn't actually reachable. Without this
/// the suite would silently pass garbage: `resolve_or_claim` degrades to
/// a local resolution (is_local=true, fence=None) when Redis is down, so
/// the exclusivity assertions could be misread.
async fn assert_redis_reachable() {
    let client = redis::Client::open(redis_url()).expect("open redis client");
    let mut conn = client.get_multiplexed_async_connection().await.expect(
        "TEST_REDIS_URL unreachable; is the dev redis up and published on 127.0.0.1:63799?",
    );
    let pong: String = redis::cmd("PING")
        .query_async(&mut conn)
        .await
        .expect("redis PING failed");
    assert_eq!(pong, "PONG");
}

fn manager(machine: &str) -> CollabOwnership {
    CollabOwnership::new(&redis_url(), Arc::from(machine), None).expect("connect to TEST_REDIS_URL")
}

fn unique_doc() -> String {
    format!("itest-{}", uuid::Uuid::new_v4())
}

/// The first machine to claim an unowned doc wins; a second machine sees
/// it as owned-elsewhere. The winner gets a fence token; the loser does
/// not (it is not the writer).
#[actix_web::test]
async fn claim_is_exclusive_across_machines() {
    assert_redis_reachable().await;
    let a = manager("machine-a");
    let b = manager("machine-b");
    let doc = unique_doc();

    let ra = a.resolve_or_claim(&doc).await;
    assert!(ra.is_local, "first claimant should own the doc locally");
    assert_eq!(ra.owner, "machine-a");
    assert!(
        ra.fence.is_some(),
        "owner must get a fence token (None here usually means Redis was unreachable)"
    );

    let rb = b.resolve_or_claim(&doc).await;
    assert!(!rb.is_local, "second machine must not own a held doc");
    assert_eq!(rb.owner, "machine-a", "loser should see the real owner");
    assert!(
        rb.fence.is_none(),
        "non-owner must not receive a fence token"
    );
}

/// A machine re-resolving a doc it already owns gets the same fence back
/// (no spurious increment), so its in-memory state keeps a stable token.
#[actix_web::test]
async fn re_resolve_by_owner_keeps_same_fence() {
    assert_redis_reachable().await;
    let a = manager("machine-a");
    let doc = unique_doc();

    let first = a.resolve_or_claim(&doc).await;
    let second = a.resolve_or_claim(&doc).await;
    assert!(first.is_local && second.is_local);
    assert_eq!(
        first.fence, second.fence,
        "re-resolving an owned doc must not bump the fence"
    );
}

/// Renewal is ownership-aware: the owner keeps the lease, a non-owner
/// cannot extend it. This is what backs lost-lease eviction.
#[actix_web::test]
async fn renew_keeps_owner_and_loser_cannot() {
    assert_redis_reachable().await;
    let a = manager("machine-a");
    let b = manager("machine-b");
    let doc = unique_doc();

    a.resolve_or_claim(&doc).await;
    assert!(a.renew(&doc).await, "owner renewal should succeed");
    assert!(
        !b.renew(&doc).await,
        "a non-owner must not be able to renew (would mask a lost lease)"
    );
}

/// Releasing frees the doc for another machine to claim, and the fence
/// token keeps climbing across the handoff so a stale prior owner is
/// always below the new one.
#[actix_web::test]
async fn release_allows_reclaim_with_higher_fence() {
    assert_redis_reachable().await;
    let a = manager("machine-a");
    let b = manager("machine-b");
    let doc = unique_doc();

    let ra = a.resolve_or_claim(&doc).await;
    let fence_a = ra.fence.expect("owner fence");

    a.release(&doc).await;

    let rb = b.resolve_or_claim(&doc).await;
    assert!(
        rb.is_local,
        "after release, the next machine should claim it"
    );
    assert_eq!(rb.owner, "machine-b");
    let fence_b = rb.fence.expect("new owner fence");
    assert!(
        fence_b > fence_a,
        "fence must increase across ownership handoff ({fence_b} should exceed {fence_a})"
    );
}

/// Fence counters are per-document: claiming one doc does not advance
/// another's token.
#[actix_web::test]
async fn fence_is_per_document() {
    assert_redis_reachable().await;
    let a = manager("machine-a");
    let doc1 = unique_doc();
    let doc2 = unique_doc();

    let f1 = a.resolve_or_claim(&doc1).await.fence.expect("doc1 fence");
    let f2 = a.resolve_or_claim(&doc2).await.fence.expect("doc2 fence");
    // Each doc's counter starts independently, so a fresh claim on a
    // never-seen doc is the first token for that doc regardless of other
    // docs' history.
    assert_eq!(
        f1, f2,
        "first claim of each distinct doc gets its own fresh fence"
    );
}
