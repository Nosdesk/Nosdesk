//! Route-registration guard for the `main.rs` route decomposition.
//!
//! As domain routes move from the `main.rs` `App` builder into per-domain
//! `handlers::<domain>::config` functions, these probes assert every route each
//! `config` registers still resolves. A registered route returns 401/4xx/5xx
//! from its auth/DB extractors (or an app-data error), never 404 — so a 404
//! means a route was dropped or renamed during extraction. `{param}` segments
//! accept any value, so the probes use placeholders.
//!
//! These live centrally (rather than one per handler module) so the whole route
//! surface is verifiable in one place; they use the crate-internal
//! `test_helpers`, so they must be unit tests, not integration tests.

use crate::test_helpers::assert_config_registers;

#[actix_web::test]
async fn sse_config_routes_registered() {
    assert_config_registers(crate::handlers::sse::config, &[("POST", "/events/token")]).await;
}

#[actix_web::test]
async fn search_config_routes_registered() {
    assert_config_registers(
        crate::handlers::search::config,
        &[
            ("GET", "/search"),
            ("POST", "/search/rebuild"),
            ("GET", "/search/stats"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn notifications_config_routes_registered() {
    assert_config_registers(
        crate::handlers::notifications::config,
        &[
            ("GET", "/notifications"),
            ("GET", "/notifications/count"),
            ("POST", "/notifications/read"),
            ("POST", "/notifications/read-all"),
            ("GET", "/notifications/preferences"),
            ("PUT", "/notifications/preferences"),
            ("POST", "/notifications/delete"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn bug_reports_config_routes_registered() {
    assert_config_registers(
        crate::handlers::bug_reports::config,
        &[("POST", "/bug-reports")],
    )
    .await;
}

#[actix_web::test]
async fn tickets_config_routes_registered() {
    assert_config_registers(
        crate::handlers::tickets::config,
        &[
            ("GET", "/tickets"),
            ("GET", "/tickets/paginated"),
            ("GET", "/tickets/recent"),
            ("POST", "/tickets"),
            ("POST", "/tickets/empty"),
            ("POST", "/tickets/bulk"),
            ("POST", "/tickets/merge"),
            ("GET", "/tickets/1/merge-history"),
            ("GET", "/tickets/1/rule-applications"),
            ("GET", "/tickets/1/applicable-actions"),
            ("GET", "/tickets/1"),
            ("PUT", "/tickets/1"),
            ("PATCH", "/tickets/1"),
            ("DELETE", "/tickets/1"),
            ("POST", "/tickets/1/view"),
            ("DELETE", "/tickets/1/view"),
            ("GET", "/tickets/1/activity"),
            ("GET", "/tickets/1/loans"),
            ("POST", "/tickets/1/field-preview"),
            ("PUT", "/tickets/1/tags"),
            ("GET", "/tickets/1/watchers"),
            ("POST", "/tickets/1/watch"),
            ("DELETE", "/tickets/1/watch"),
            ("GET", "/tickets/1/watch/me"),
            ("PATCH", "/tickets/1/watch/preferences"),
            ("GET", "/tags"),
            ("POST", "/tags"),
            ("PATCH", "/tags/1"),
            ("DELETE", "/tags/1"),
            ("POST", "/import/file"),
            ("POST", "/import/json"),
            ("POST", "/tickets/1/link/1"),
            ("DELETE", "/tickets/1/unlink/1"),
            ("POST", "/tickets/1/assets/1"),
            ("DELETE", "/tickets/1/assets/1"),
            ("GET", "/tickets/1/asset-usage"),
            ("GET", "/tickets/1/comments"),
            ("POST", "/tickets/1/comments"),
            ("POST", "/tickets/1/notes/images"),
            ("DELETE", "/comments/1"),
            ("GET", "/comments/1/raw.eml"),
            ("GET", "/image-proxy/1/1"),
            ("POST", "/comments/1/attachments"),
            ("DELETE", "/attachments/1"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn projects_config_routes_registered() {
    assert_config_registers(
        crate::handlers::projects::config,
        &[
            ("GET", "/projects"),
            ("POST", "/projects"),
            ("GET", "/projects/1"),
            ("PUT", "/projects/1"),
            ("DELETE", "/projects/1"),
            ("GET", "/projects/1/tickets"),
            ("GET", "/projects/1/dependencies"),
            ("POST", "/projects/1/tickets/new"),
            ("POST", "/projects/1/tickets/1"),
            ("DELETE", "/projects/1/tickets/1"),
            ("PUT", "/projects/1/tickets/order"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn groups_config_routes_registered() {
    assert_config_registers(
        crate::handlers::groups::config,
        &[
            ("GET", "/groups/details/1"),
            ("GET", "/groups"),
            ("POST", "/groups"),
            ("GET", "/groups/1"),
            ("PUT", "/groups/1"),
            ("DELETE", "/groups/1"),
            ("PUT", "/groups/1/members"),
            ("PUT", "/groups/1/assets"),
            ("GET", "/groups/1/includes"),
            ("PUT", "/groups/1/includes"),
            ("POST", "/groups/1/unmanage"),
            ("GET", "/users/1/groups"),
            ("PUT", "/users/1/groups"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn categories_config_routes_registered() {
    assert_config_registers(
        crate::handlers::categories::config,
        &[
            ("GET", "/categories"),
            ("GET", "/admin/categories"),
            ("POST", "/admin/categories"),
            ("PUT", "/admin/categories/reorder"),
            ("GET", "/admin/categories/1"),
            ("PUT", "/admin/categories/1"),
            ("DELETE", "/admin/categories/1"),
            ("PUT", "/admin/categories/1/visibility"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn assignment_rules_config_routes_registered() {
    assert_config_registers(
        crate::handlers::assignment_rules::config,
        &[
            ("GET", "/admin/assignment-rules"),
            ("POST", "/admin/assignment-rules"),
            ("PUT", "/admin/assignment-rules/reorder"),
            ("POST", "/admin/assignment-rules/preview"),
            ("GET", "/admin/assignment-rules/logs"),
            ("GET", "/admin/assignment-rules/1"),
            ("PATCH", "/admin/assignment-rules/1"),
            ("DELETE", "/admin/assignment-rules/1"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn api_tokens_config_routes_registered() {
    assert_config_registers(
        crate::handlers::api_tokens::config,
        &[
            ("GET", "/admin/api-tokens"),
            ("POST", "/admin/api-tokens"),
            ("GET", "/admin/api-tokens/1"),
            ("DELETE", "/admin/api-tokens/1"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn plugins_config_routes_registered() {
    assert_config_registers(
        crate::handlers::plugins::config,
        &[
            ("GET", "/admin/plugins"),
            ("GET", "/admin/plugins/config"),
            ("GET", "/admin/plugins/signing-overview"),
            ("POST", "/admin/plugins/install"),
            ("GET", "/admin/plugins/registry"),
            ("POST", "/admin/plugins/registry/refresh"),
            ("POST", "/admin/plugins/registry/install"),
            ("GET", "/admin/plugins/1"),
            ("PUT", "/admin/plugins/1"),
            ("DELETE", "/admin/plugins/1"),
            ("GET", "/admin/plugins/1/settings"),
            ("POST", "/admin/plugins/1/settings"),
            ("DELETE", "/admin/plugins/1/settings/1"),
            ("GET", "/admin/plugins/1/activity"),
            ("GET", "/plugins/enabled"),
            ("GET", "/plugins/1/bundle"),
            ("GET", "/plugins/1/icon"),
            ("GET", "/plugins/1/storage/1"),
            ("POST", "/plugins/1/storage"),
            ("DELETE", "/plugins/1/storage/1"),
            ("POST", "/plugins/1/proxy"),
            ("POST", "/plugins/1/events"),
            ("GET", "/plugins/1/collections"),
            ("GET", "/plugins/1/collections/1"),
            ("GET", "/plugins/1/collections/1/rows"),
            ("POST", "/plugins/1/collections/1/rows"),
            ("GET", "/plugins/1/collections/1/rows/1"),
            ("PUT", "/plugins/1/collections/1/rows/1"),
            ("DELETE", "/plugins/1/collections/1/rows/1"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn users_config_routes_registered() {
    assert_config_registers(
        crate::handlers::users::config,
        &[
            ("GET", "/users"),
            ("GET", "/users/paginated"),
            ("POST", "/users/batch"),
            ("POST", "/users/bulk"),
            ("POST", "/users/cleanup-images"),
            ("POST", "/users/regenerate-thumbnails"),
            ("POST", "/files/cleanup-temp"),
            ("GET", "/users/auth-identities"),
            ("DELETE", "/users/auth-identities/1"),
            ("POST", "/users"),
            ("GET", "/users/1"),
            ("PUT", "/users/1"),
            ("DELETE", "/users/1"),
            ("POST", "/users/1/restore"),
            ("DELETE", "/users/1/purge"),
            ("POST", "/users/1/image"),
            ("GET", "/users/1/emails"),
            ("POST", "/users/1/emails"),
            ("PUT", "/users/1/emails/1"),
            ("DELETE", "/users/1/emails/1"),
            ("GET", "/users/1/profile-fields"),
            ("PUT", "/users/1/profile-fields"),
            ("GET", "/users/1/phones"),
            ("POST", "/users/1/phones"),
            ("PUT", "/users/1/phones/1"),
            ("DELETE", "/users/1/phones/1"),
            ("GET", "/users/1/addresses"),
            ("POST", "/users/1/addresses"),
            ("PUT", "/users/1/addresses/1"),
            ("DELETE", "/users/1/addresses/1"),
            ("GET", "/admin/user-fields"),
            ("PUT", "/admin/user-fields"),
            ("GET", "/ldap/settings"),
            ("GET", "/ldap/sync-history"),
            ("PUT", "/ldap/settings"),
            ("GET", "/ldap/presets"),
            ("POST", "/ldap/test-connection"),
            ("GET", "/ldap/discover-groups"),
            ("POST", "/ldap/preview"),
            ("POST", "/ldap/sync"),
            ("GET", "/users/1/with-emails"),
            ("GET", "/users/1/profile"),
            ("GET", "/users/1/auth-identities"),
            ("DELETE", "/users/1/auth-identities/1"),
            ("POST", "/users/1/resend-invitation"),
            ("GET", "/users/1/security-info"),
            ("POST", "/users/1/reset-password"),
            ("POST", "/users/1/disable-mfa"),
            ("DELETE", "/users/1/passkeys/1"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn assets_config_routes_registered() {
    assert_config_registers(
        crate::handlers::assets::config,
        &[
            ("GET", "/assets"),
            ("GET", "/assets/paginated"),
            ("GET", "/assets/paginated/excluding"),
            ("POST", "/assets/bulk"),
            ("GET", "/assets/calendar-overlay"),
            ("GET", "/assets/export"),
            ("GET", "/assets/locations"),
            ("GET", "/assets/itad-vendors"),
            ("GET", "/assets/grouping-dataset"),
            ("POST", "/assets/rollouts"),
            ("GET", "/asset-kinds"),
            ("GET", "/manufacturers"),
            ("POST", "/manufacturers"),
            ("GET", "/manufacturers/1"),
            ("PUT", "/manufacturers/1"),
            ("DELETE", "/manufacturers/1"),
            ("GET", "/asset-models"),
            ("POST", "/asset-models"),
            ("GET", "/asset-models/1"),
            ("PUT", "/asset-models/1"),
            ("DELETE", "/asset-models/1"),
            ("GET", "/asset-groups"),
            ("POST", "/asset-groups"),
            ("PUT", "/asset-groups/1"),
            ("POST", "/asset-groups/1/archive"),
            ("POST", "/asset-groups/1/restore"),
            ("POST", "/assets"),
            ("POST", "/assets/empty"),
            ("POST", "/assets/1/model"),
            ("DELETE", "/assets/1/model"),
            ("GET", "/assets/1"),
            ("PUT", "/assets/1"),
            ("DELETE", "/assets/1"),
            ("POST", "/assets/1/unmanage"),
            ("GET", "/assets/1/lifecycle"),
            ("POST", "/assets/1/lifecycle"),
            ("GET", "/assets/1/disposal"),
            ("GET", "/assets/1/record-card"),
            ("GET", "/assets/1/loans"),
            ("POST", "/assets/1/loans"),
            ("PATCH", "/assets/1/loans/1"),
            ("POST", "/assets/1/loans/1/return"),
            ("GET", "/assets/1/media"),
            ("POST", "/assets/1/media"),
            ("PUT", "/assets/1/media/1"),
            ("DELETE", "/assets/1/media/1"),
            ("POST", "/assets/1/usage"),
            ("GET", "/assets/1/usage"),
            ("POST", "/assets/1/audit"),
            ("GET", "/assets/1/audits"),
            ("PUT", "/assets/1/groups"),
            ("GET", "/users/1/assets"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn documentation_config_routes_registered() {
    assert_config_registers(
        crate::handlers::documentation::config,
        &[
            ("GET", "/documentation/pages"),
            ("POST", "/documentation/pages"),
            ("GET", "/documentation/pages/export"),
            ("GET", "/documentation/pages/top-level"),
            ("GET", "/documentation/pages/uncollected"),
            ("POST", "/documentation/pages/reorder"),
            ("POST", "/documentation/pages/move"),
            ("GET", "/documentation/pages/ordered/top-level"),
            ("GET", "/documentation/pages/ordered/parent/1"),
            ("GET", "/documentation/pages/parent/1"),
            ("GET", "/documentation/pages/uuid/1/content"),
            ("GET", "/documentation/pages/slug/1"),
            ("GET", "/documentation/pages/slug/1/with-children"),
            ("GET", "/documentation/pages/archived"),
            ("GET", "/documentation/pages/trash"),
            ("GET", "/documentation/starred"),
            ("GET", "/documentation/pages/1"),
            ("PUT", "/documentation/pages/1"),
            ("DELETE", "/documentation/pages/1"),
            ("GET", "/documentation/pages/1/with-children-by-parent"),
            ("GET", "/documentation/pages/1/with-ordered-children"),
            ("PUT", "/documentation/pages/1/embeddings"),
            ("GET", "/documentation/pages/1/export/markdown"),
            ("GET", "/documentation/pages/1/collections"),
            ("PUT", "/documentation/pages/1/collections"),
            ("GET", "/documentation/pages/1/visibility"),
            ("PUT", "/documentation/pages/1/visibility"),
            ("GET", "/documentation/pages/1/subscription"),
            ("POST", "/documentation/pages/1/subscribe"),
            ("DELETE", "/documentation/pages/1/subscribe"),
            ("GET", "/documentation/pages/1/starred"),
            ("POST", "/documentation/pages/1/star"),
            ("DELETE", "/documentation/pages/1/star"),
            ("POST", "/documentation/pages/1/restore"),
            ("DELETE", "/documentation/pages/1/permanent"),
            ("GET", "/tickets/1/documentation"),
            ("POST", "/tickets/1/documentation/create"),
            ("POST", "/tickets/1/flag-as-gap"),
            ("DELETE", "/tickets/1/flag-as-gap"),
            ("GET", "/knowledge-gaps"),
            ("POST", "/knowledge-gaps/detect-clusters"),
            ("POST", "/knowledge-gaps/detect-failed-searches"),
            ("POST", "/knowledge-gaps/detect-stale-docs"),
            ("GET", "/knowledge-gaps/1"),
            ("POST", "/knowledge-gaps/1/dismiss"),
            ("POST", "/knowledge-gaps/1/resolve"),
            ("GET", "/tickets/1/documentation-pages"),
            ("GET", "/documentation/pages/1/tickets"),
            ("POST", "/documentation/pages/1/tickets"),
            ("DELETE", "/documentation/pages/1/tickets/1"),
            ("POST", "/documentation/pages/1/verification"),
            ("DELETE", "/documentation/pages/1/verification"),
        ],
    )
    .await;
}

#[actix_web::test]
async fn documentation_collections_config_routes_registered() {
    assert_config_registers(
        crate::handlers::documentation_collections::config,
        &[
            ("GET", "/documentation/collections"),
            ("POST", "/documentation/collections"),
            ("POST", "/documentation/collections/reorder"),
            ("GET", "/documentation/collections/slug/1"),
            ("GET", "/documentation/collections/1"),
            ("PUT", "/documentation/collections/1"),
            ("DELETE", "/documentation/collections/1"),
            ("POST", "/documentation/collections/1/pages"),
            ("DELETE", "/documentation/collections/1/pages/1"),
            ("GET", "/documentation/collections/1/visibility"),
            ("PUT", "/documentation/collections/1/visibility"),
            ("GET", "/documentation/collections/1/page-overrides"),
            ("PUT", "/documentation/1"),
            ("DELETE", "/documentation/1"),
        ],
    )
    .await;
}
