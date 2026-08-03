use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;
use yrs::types::xml::{XmlDeltaPrelim, XmlIn};
use yrs::types::Delta;
use yrs::{Any, Doc, ReadTxn, Transact, WriteTxn, XmlElementPrelim, XmlFragment};

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
/// each sub-seed no-ops when its rows already exist.
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
    Ok(())
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

/// Convert a simple markdown string to a Yjs document binary (V1 encoded).
///
/// Builds a y-prosemirror compatible XmlFragment("prosemirror") with proper
/// block structure (paragraph, heading, bullet_list, etc.) and inline marks
/// (bold, italic, code) stored as text formatting attributes via XmlDeltaPrelim.
///
/// Public so the `seed_demo` binary can render demo ticket bodies through the
/// same converter used for the welcome page.
pub fn markdown_to_yjs(markdown: &str) -> Option<Vec<u8>> {
    let doc = Doc::new();
    let fragment = {
        let mut txn = doc.transact_mut();
        txn.get_or_insert_xml_fragment("prosemirror")
    };

    let mut txn = doc.transact_mut();

    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Heading
        if let Some(heading) = parse_heading(line) {
            let delta = parse_inline_to_delta(heading.text);
            let mut elem = XmlElementPrelim::new("heading", vec![delta.into()]);
            elem.attributes
                .insert("level".into(), heading.level.to_string());
            fragment.push_back(&mut txn, elem);
            i += 1;
            continue;
        }

        // Unordered list
        if line.trim_start().starts_with("- ") {
            let mut list_items: Vec<XmlIn> = Vec::new();

            while i < lines.len() && lines[i].trim_start().starts_with("- ") {
                let item_text = lines[i].trim_start().trim_start_matches("- ");
                let delta = parse_inline_to_delta(item_text);
                let para = XmlElementPrelim::new("paragraph", vec![delta.into()]);
                let li = XmlElementPrelim::new("list_item", vec![para.into()]);
                list_items.push(li.into());
                i += 1;
            }

            let ul = XmlElementPrelim::new("bullet_list", list_items);
            fragment.push_back(&mut txn, ul);
            continue;
        }

        // Ordered list
        if line
            .trim_start()
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && line.trim_start().contains(". ")
        {
            let mut list_items: Vec<XmlIn> = Vec::new();

            while i < lines.len() {
                let l = lines[i].trim_start();
                if l.chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && l.contains(". ")
                {
                    let item_text = l.split_once(". ").map(|x| x.1).unwrap_or("");
                    let delta = parse_inline_to_delta(item_text);
                    let para = XmlElementPrelim::new("paragraph", vec![delta.into()]);
                    let li = XmlElementPrelim::new("list_item", vec![para.into()]);
                    list_items.push(li.into());
                    i += 1;
                } else {
                    break;
                }
            }

            let ol = XmlElementPrelim::new("ordered_list", list_items);
            fragment.push_back(&mut txn, ol);
            continue;
        }

        // Horizontal rule
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            let hr = XmlElementPrelim::empty("horizontal_rule");
            fragment.push_back(&mut txn, hr);
            i += 1;
            continue;
        }

        // Default: paragraph
        let delta = parse_inline_to_delta(line);
        let para = XmlElementPrelim::new("paragraph", vec![delta.into()]);
        fragment.push_back(&mut txn, para);
        i += 1;
    }

    drop(txn);

    let txn = doc.transact();
    let update = txn.encode_state_as_update_v1(&yrs::StateVector::default());
    Some(update)
}

struct Heading<'a> {
    level: usize,
    text: &'a str,
}

fn parse_heading(line: &str) -> Option<Heading<'_>> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }

    let rest = &trimmed[level..];
    if !rest.starts_with(' ') {
        return None;
    }

    Some(Heading {
        level,
        text: rest.trim(),
    })
}

/// Represents an inline text span with optional mark.
enum InlineSpan<'a> {
    Plain(&'a str),
    Bold(&'a str),
    Italic(&'a str),
    Code(&'a str),
    /// `[text](href)` — carries the destination through as a real link mark.
    Link {
        text: &'a str,
        href: &'a str,
    },
}

/// Parse inline markdown into an XmlDeltaPrelim with proper text formatting attributes.
///
/// In y-prosemirror, marks (bold/italic/code) are stored as formatting attributes
/// on XmlText delta operations, not as XML wrapper elements. For example, bold text
/// is stored as `Delta::insert_with("text", { "strong": {} })`.
///
/// Mark type names must match the ProseMirror schema keys exactly:
/// bold = "strong", italic = "em", inline code = "code".
fn parse_inline_to_delta(text: &str) -> XmlDeltaPrelim {
    let spans = parse_inline_spans(text);
    let mut delta: Vec<Delta<yrs::In>> = Vec::new();

    for span in spans {
        match span {
            InlineSpan::Plain(s) => {
                delta.push(Delta::insert(s));
            }
            InlineSpan::Bold(s) => {
                let attrs = HashMap::from([(Arc::from("strong"), Any::Bool(true))]);
                delta.push(Delta::insert_with(s, attrs));
            }
            InlineSpan::Italic(s) => {
                let attrs = HashMap::from([(Arc::from("em"), Any::Bool(true))]);
                delta.push(Delta::insert_with(s, attrs));
            }
            InlineSpan::Code(s) => {
                let attrs = HashMap::from([(Arc::from("code"), Any::Bool(true))]);
                delta.push(Delta::insert_with(s, attrs));
            }
            InlineSpan::Link { text, href } => {
                // Unlike the boolean marks, `link` carries attributes, so the
                // mark value is the ProseMirror attrs map (schema: href
                // required, title optional) rather than `true`.
                let link_attrs = Any::from(HashMap::from([(
                    "href".to_string(),
                    Any::String(href.into()),
                )]));
                let attrs = HashMap::from([(Arc::from("link"), link_attrs)]);
                delta.push(Delta::insert_with(text, attrs));
            }
        }
    }

    // Ensure at least one delta entry (empty paragraph)
    if delta.is_empty() {
        delta.push(Delta::insert(""));
    }

    XmlDeltaPrelim {
        attributes: HashMap::new(),
        delta,
    }
}

/// Parse a line of text into inline spans, recognizing **bold**, *italic*, `code`,
/// and [link text](href).
fn parse_inline_spans(text: &str) -> Vec<InlineSpan<'_>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find(['*', '`', '[']) {
            // Plain text before marker
            if pos > 0 {
                spans.push(InlineSpan::Plain(&remaining[..pos]));
            }

            let after = &remaining[pos..];

            // Bold: **text**
            if after.starts_with("**") {
                if let Some(end) = after[2..].find("**") {
                    spans.push(InlineSpan::Bold(&after[2..2 + end]));
                    remaining = &after[2 + end + 2..];
                    continue;
                }
            }

            // Italic: *text* (but not **)
            if after.starts_with('*') && !after.starts_with("**") {
                if let Some(end) = after[1..].find('*') {
                    spans.push(InlineSpan::Italic(&after[1..1 + end]));
                    remaining = &after[1 + end + 1..];
                    continue;
                }
            }

            // Inline code: `text`
            if after.starts_with('`') {
                if let Some(end) = after[1..].find('`') {
                    spans.push(InlineSpan::Code(&after[1..1 + end]));
                    remaining = &after[1 + end + 1..];
                    continue;
                }
            }

            // Link: [text](href)
            if after.starts_with('[') {
                if let Some(bracket_end) = after.find(']') {
                    let link_text = &after[1..bracket_end];
                    let after_bracket = &after[bracket_end + 1..];
                    if after_bracket.starts_with('(') {
                        if let Some(paren_end) = after_bracket.find(')') {
                            let href = &after_bracket[1..paren_end];
                            // An empty href would produce a link mark with no
                            // destination; fall back to plain text.
                            if href.is_empty() {
                                spans.push(InlineSpan::Plain(link_text));
                            } else {
                                spans.push(InlineSpan::Link {
                                    text: link_text,
                                    href,
                                });
                            }
                            remaining = &after_bracket[paren_end + 1..];
                            continue;
                        }
                    }
                }
            }

            // No pattern matched - treat character as literal
            spans.push(InlineSpan::Plain(&remaining[pos..pos + 1]));
            remaining = &remaining[pos + 1..];
        } else {
            spans.push(InlineSpan::Plain(remaining));
            break;
        }
    }

    spans
}

#[cfg(test)]
mod inline_tests {
    use super::{parse_inline_spans, InlineSpan};

    /// The seeded welcome page links out to the public docs, so the converter
    /// has to carry the destination through. It used to drop the href and
    /// render link text as plain text.
    #[test]
    fn link_keeps_its_href() {
        let spans = parse_inline_spans("see the [user guide](https://nosdesk.com/docs/guide) now");
        let link = spans
            .iter()
            .find_map(|s| match s {
                InlineSpan::Link { text, href } => Some((*text, *href)),
                _ => None,
            })
            .expect("a link span");
        assert_eq!(link, ("user guide", "https://nosdesk.com/docs/guide"));
    }

    #[test]
    fn empty_href_falls_back_to_plain_text() {
        let spans = parse_inline_spans("[bare]()");
        assert!(
            !spans.iter().any(|s| matches!(s, InlineSpan::Link { .. })),
            "an empty href must not produce a destination-less link mark"
        );
    }

    /// End-to-end guard on the shipped welcome page: it converts, and the
    /// public-docs links survive into the encoded document. Fails both if the
    /// converter drops hrefs again and if the seed file loses its links.
    #[test]
    fn seeded_welcome_page_encodes_real_links() {
        let markdown = include_str!("../../seeds/getting-started.md");
        let bytes = super::markdown_to_yjs(markdown).expect("welcome markdown converts");
        let encoded = String::from_utf8_lossy(&bytes);
        assert!(encoded.contains("link"), "link mark is encoded");
        assert!(
            encoded.contains("https://nosdesk.com/docs/guide/ticket-queue"),
            "the href is carried into the document, not dropped"
        );
    }

    #[test]
    fn other_inline_marks_still_parse() {
        let spans = parse_inline_spans("**b** *i* `c`");
        assert!(spans.iter().any(|s| matches!(s, InlineSpan::Bold("b"))));
        assert!(spans.iter().any(|s| matches!(s, InlineSpan::Italic("i"))));
        assert!(spans.iter().any(|s| matches!(s, InlineSpan::Code("c"))));
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
