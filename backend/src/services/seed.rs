use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    DocumentationCollection, DocumentationStatus, NewDocumentationCollection,
    NewDocumentationCollectionPage, NewDocumentationPage,
};
use crate::repository;
use crate::repository::documentation_collections;
use crate::utils::i18n;
use crate::utils::locale::DEFAULT_LOCALE;
use diesel::QueryResult;
use std::str::FromStr;
use unic_langid::LanguageIdentifier;

/// Run all seed checks on startup. Targets the bootstrap workspace (the
/// caller pins the actor context to workspace 1), whose workflow states /
/// SLA / categories / Getting Started collection already exist from the
/// initial migration, so only the welcome page needs seeding here.
/// Each seed is idempotent - it only creates content if it doesn't already exist.
pub fn run_seeds(conn: &mut DbConnection) {
    // The welcome page's author columns are NOT NULL with an FK to
    // `users`, so the docs seed only runs when a platform admin exists to
    // author it. On a clean install the bootstrap admin is created first,
    // so this is satisfied by the time startup seeding runs.
    match first_platform_admin(conn) {
        Some(author) => {
            if let Err(e) = seed_getting_started(conn, author) {
                warn!(error = %e, "Failed to seed Getting Started welcome page");
            }
        }
        None => debug!("No platform admin yet; skipping Getting Started welcome-page seed"),
    }
}

/// Seed the functional defaults a freshly-provisioned workspace needs to be
/// usable: workflow states (the ticket-creation blocker), a working
/// calendar + SLA policy, and ticket categories. Idempotent per workspace;
/// each sub-seed no-ops when its rows already exist. On hosted, the portal and
/// emails are also named after the workspace (self-hosted keeps "Nosdesk" until
/// an admin names them).
///
/// Starter docs are deliberately NOT seeded here: the welcome page's author
/// columns are NOT NULL with an FK to `users`, and at hosted create time no
/// user exists yet (`created_by` is `None`). The Getting Started docs are
/// seeded later, at owner projection, via [`seed_getting_started`].
///
/// The caller MUST run this inside an actor context pinned to the target
/// workspace (`ActorContext::system(...).with_workspace(id)`), so the
/// `app.workspace_id` GUC drives the per-row workspace_id defaults and the
/// audit triggers attribute the writes to that workspace.
pub fn seed_workspace_defaults(
    conn: &mut DbConnection,
    created_by: Option<Uuid>,
) -> QueryResult<()> {
    repository::workflow_states::seed_defaults_if_empty(conn, created_by)?;
    repository::sla_admin::seed_defaults_if_empty(conn, created_by)?;
    repository::categories::seed_defaults_if_empty(conn, created_by)?;
    repository::asset_kinds::seed_defaults_if_empty(conn, created_by)?;
    if crate::middleware::workspace_context::is_hosted() {
        repository::site_settings::name_after_workspace(conn)?;
    }
    Ok(())
}

/// `system_meta` key recording that [`name_hosted_workspaces`] has run.
const KEY_HOSTED_APP_NAMES: &str = "hosted_app_names_backfilled";

/// Hosted, once: name every workspace whose portal and emails still say
/// "Nosdesk" after the workspace. Workspaces provisioned before naming moved
/// into [`seed_workspace_defaults`] were left on the default; later ones are
/// named at create. Records completion in `system_meta` and does nothing on
/// later boots, so an admin who sets "Nosdesk" afterwards keeps it. A workspace
/// admins had already named is never touched, nor is the bootstrap workspace.
/// If any workspace fails, completion isn't recorded and the next boot retries.
/// Returns how many it named.
pub fn name_hosted_workspaces(pool: &crate::db::Pool) -> usize {
    if !crate::middleware::workspace_context::is_hosted() {
        return 0;
    }
    // cross-tenant: reads the global system_meta marker, then lists every
    // workspace whose settings are still unnamed.
    let pending =
        match crate::sync::session::background_run(pool, "startup:hosted_app_name", |conn| {
            if crate::sync::system_meta::get(conn, KEY_HOSTED_APP_NAMES)?.is_some() {
                return Ok(None);
            }
            repository::site_settings::workspaces_with_default_app_name(conn).map(Some)
        }) {
            Ok(Some(ids)) => ids,
            Ok(None) => return 0,
            Err(e) => {
                warn!(error = %e, "Could not list workspaces still named Nosdesk");
                return 0;
            }
        };
    let mut named = 0;
    let mut failed = false;
    for workspace_id in pending {
        match crate::sync::session::run_in_workspace(
            pool,
            "startup:hosted_app_name",
            workspace_id,
            repository::site_settings::name_after_workspace,
        ) {
            Ok(true) => named += 1,
            Ok(false) => {}
            Err(e) => {
                failed = true;
                warn!(workspace_id, error = %e, "Could not name a workspace's portal");
            }
        }
    }
    if named > 0 {
        info!(
            count = named,
            "Named hosted workspaces' portals after the workspace"
        );
    }
    if !failed {
        // cross-tenant: system_meta is a global table.
        let recorded =
            crate::sync::session::background_run(pool, "startup:hosted_app_name", |conn| {
                crate::sync::system_meta::put(
                    conn,
                    KEY_HOSTED_APP_NAMES,
                    &serde_json::json!({ "named": named }),
                )
            });
        if let Err(e) = recorded {
            warn!(error = %e, "Could not record that hosted workspaces were named");
        }
    }
    named
}

/// Resolve the system user to credit seed content to: the first platform
/// admin, or `None` when no admin exists yet (e.g. a hosted workspace
/// seeded before its owner is projected).
fn first_platform_admin(conn: &mut DbConnection) -> Option<Uuid> {
    use crate::schema::users;
    use diesel::prelude::*;
    users::table
        .filter(users::platform_role.eq("platform_admin"))
        .filter(users::deleted_at.is_null())
        .select(users::uuid)
        .first::<Uuid>(conn)
        .ok()
}

/// Get-or-create the workspace's "Getting Started" system collection. A
/// freshly-provisioned workspace has none (the slug is unique per
/// workspace, so this never collides with the bootstrap workspace's row).
fn ensure_getting_started_collection(
    conn: &mut DbConnection,
    author: Uuid,
) -> QueryResult<DocumentationCollection> {
    match documentation_collections::get_collection_by_slug(conn, "getting-started") {
        Ok(c) => Ok(c),
        Err(diesel::result::Error::NotFound) => documentation_collections::create_collection(
            conn,
            NewDocumentationCollection {
                uuid: Uuid::new_v4(),
                name: "Getting Started".to_string(),
                slug: "getting-started".to_string(),
                description: Some("Introduction and onboarding documentation".to_string()),
                icon: Some("\u{1F680}".to_string()),
                color: None,
                is_system: true,
                created_by: Some(author),
            },
        ),
        Err(e) => Err(e),
    }
}

/// Seed the workspace's "Getting Started" collection with a welcome page if
/// it's empty, authored by `author`. Idempotent: a no-op once the
/// collection has any page. `author` is required (not optional) because the
/// page's author columns are NOT NULL with an FK to `users`, so this can
/// only run once a real user exists for the workspace (the owner, at
/// projection time; or the bootstrap admin, at install).
///
/// Returns `Ok` (a no-op) when the page can't be built from the embedded
/// markdown — a Yjs-conversion hiccup must never block provisioning — and
/// only propagates genuine DB errors. Caller must run inside an actor
/// context pinned to the target workspace.
pub fn seed_getting_started(conn: &mut DbConnection, author: Uuid) -> QueryResult<()> {
    let collection = ensure_getting_started_collection(conn, author)?;

    // Check if it already has pages
    let pages = documentation_collections::get_pages_in_collection(conn, collection.id)?;
    if !pages.is_empty() {
        debug!(
            "Getting Started collection already has {} pages, skipping seed",
            pages.len()
        );
        return Ok(());
    }

    // The seed markdown is embedded at compile time via include_str!().
    // The earlier CWD-relative read (seeds/getting-started.md plus a
    // backend/ fallback) was fragile: it worked under the dev container
    // because CWD happened to be /app, but failed under slimmer images,
    // host-side `cargo run` from outside backend/, and any future bin
    // that didn't happen to launch from the right directory. Embedding
    // makes the seed install-relative-by-construction — wherever the
    // binary runs, the content is in it.
    let markdown = include_str!("../../seeds/getting-started.md");

    // Convert markdown to a Yjs document. A conversion failure is
    // non-fatal: skip the page rather than abort provisioning.
    let yjs_document = match markdown_to_yjs(markdown) {
        Some(doc) => doc,
        None => {
            warn!("Failed to convert getting-started.md to Yjs document");
            return Ok(());
        }
    };

    // Create the welcome page. The seed runs with no per-user locale
    // context, so resolve the title against DEFAULT_LOCALE; admin can
    // rename it afterwards via the documentation editor.
    let seed_locale = LanguageIdentifier::from_str(DEFAULT_LOCALE).expect("DEFAULT_LOCALE parses");
    let new_page = NewDocumentationPage {
        uuid: Uuid::new_v4(),
        title: i18n::tr(&seed_locale, "seed-welcome-page-title"),
        slug: "welcome".to_string(),
        icon: Some("\u{1F44B}".to_string()),
        cover_image: None,
        status: DocumentationStatus::Published,
        created_by: author,
        last_edited_by: author,
        parent_id: None,
        display_order: Some(0),
        is_public: true,
        is_template: false,
        yjs_state_vector: None,
        yjs_document: Some(yjs_document),
        yjs_client_id: None,
        has_unsaved_changes: false,
    };

    let page = repository::create_documentation_page(new_page, conn)?;
    let entry = NewDocumentationCollectionPage {
        collection_id: collection.id,
        page_id: page.id,
        created_by: Some(author),
    };
    documentation_collections::add_page_to_collection(conn, entry)?;
    info!("Seeded welcome page in Getting Started collection");
    Ok(())
}

/// Convert Markdown to a Yjs document binary (v1 encoded) through
/// [`docs_markdown`](crate::services::docs_markdown), the converter the
/// documentation import uses, so seeded pages are written exactly as the
/// editor writes them.
///
/// Public so the `seed_demo` binary can render demo ticket bodies through the
/// same converter used for the welcome page. Always `Some`; the `Option` is
/// kept for its callers.
pub fn markdown_to_yjs(markdown: &str) -> Option<Vec<u8>> {
    Some(crate::services::docs_markdown::markdown_to_yjs(markdown).0)
}

#[cfg(test)]
mod inline_tests {
    use crate::services::docs_markdown::{convert, ir::Inline};

    /// End-to-end guard on the shipped welcome page: it converts with nothing
    /// degraded, and its public-docs links survive as link marks. Fails both
    /// if the converter drops hrefs and if the seed file loses its links.
    #[test]
    fn seeded_welcome_page_encodes_real_links() {
        let markdown = include_str!("../../seeds/getting-started.md");
        let converted = convert(markdown);
        assert!(converted.notes.is_empty(), "{:?}", converted.notes);
        let hrefs: Vec<String> = converted
            .document
            .blocks
            .iter()
            .flat_map(|b| match b {
                crate::services::docs_markdown::ir::Block::Paragraph(i) => i.clone(),
                _ => Vec::new(),
            })
            .filter_map(|i| match i {
                Inline::Text(t) => t.marks.link.map(|l| l.href),
                _ => None,
            })
            .collect();
        assert!(hrefs.contains(&"https://nosdesk.com/docs/guide/ticket-queue".to_string()));
        let bytes = super::markdown_to_yjs(markdown).expect("welcome markdown converts");
        let encoded = String::from_utf8_lossy(&bytes);
        assert!(encoded.contains("https://nosdesk.com/docs/guide/ticket-queue"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewWorkspace;
    use crate::repository::workspaces::{self, CreateWorkspaceError};
    use crate::sync::actor::ActorContext;
    use crate::sync::session::{set_actor, with_actor_bypass_context};
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use diesel::dsl::count_star;
    use diesel::prelude::*;

    /// Provision a fresh workspace the way the handler does (admin role for
    /// the `workspaces` insert, then drop to the app role with the new
    /// workspace pinned) and assert the functional defaults land, the docs
    /// seed is deferred, and re-running is idempotent.
    #[test]
    fn seed_workspace_defaults_populates_a_fresh_workspace() {
        let mut conn = setup_test_connection();
        // A real user to author the welcome page (its author columns are a
        // NOT NULL FK to the global `users` table). Created in the ambient
        // context; only its uuid matters here.
        let author = TestFixtures::create_user(&mut conn, "seed_author", "admin");

        let provision = ActorContext::system("test:seed");
        with_actor_bypass_context::<(), CreateWorkspaceError>(&mut conn, &provision, |c| {
            let record = NewWorkspace {
                uuid: Uuid::now_v7(),
                slug: "seedtest".to_string(),
                name: "Seed Test".to_string(),
                seat_limit: None,
            };
            let ws = workspaces::create_workspace(c, &record)?;

            // Drop to the app role + pin the new workspace, so the
            // RLS-scoped counts below see only this workspace's rows.
            set_actor(c, &ActorContext::system("test:seed").with_workspace(ws.id))?;
            seed_workspace_defaults(c, None)?;

            use crate::schema::{
                sla_policies, ticket_categories, workflow_states, working_calendars,
            };

            let states: i64 = workflow_states::table.select(count_star()).first(c)?;
            assert_eq!(states, 7, "7 default workflow states");
            let defaults: i64 = workflow_states::table
                .filter(workflow_states::is_default.eq(true))
                .select(count_star())
                .first(c)?;
            assert_eq!(defaults, 1, "exactly one default state");

            let calendars: i64 = working_calendars::table.select(count_star()).first(c)?;
            assert_eq!(calendars, 1, "one default working calendar");
            let policies: i64 = sla_policies::table.select(count_star()).first(c)?;
            assert_eq!(policies, 1, "one default SLA policy");

            let categories: i64 = ticket_categories::table.select(count_star()).first(c)?;
            assert_eq!(categories, 3, "three default categories");

            // Docs are NOT part of the functional create-time seed.
            assert!(
                documentation_collections::get_collection_by_slug(c, "getting-started").is_err(),
                "getting-started collection should not exist before owner projection"
            );

            // The owner-projection docs seed: authored welcome page.
            seed_getting_started(c, author.uuid)?;
            let coll = documentation_collections::get_collection_by_slug(c, "getting-started")?;
            let pages = documentation_collections::get_pages_in_collection(c, coll.id)?;
            assert_eq!(pages.len(), 1, "one welcome page after docs seed");

            // Idempotent: re-running both seeds adds nothing.
            seed_workspace_defaults(c, None)?;
            seed_getting_started(c, author.uuid)?;
            let states_again: i64 = workflow_states::table.select(count_star()).first(c)?;
            assert_eq!(states_again, 7, "workflow states not duplicated on re-seed");
            let pages_again = documentation_collections::get_pages_in_collection(c, coll.id)?;
            assert_eq!(
                pages_again.len(),
                1,
                "welcome page not duplicated on re-seed"
            );

            Ok(())
        })
        .expect("provision + seed fresh workspace");
    }
}
