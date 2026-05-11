use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, debug};
use uuid::Uuid;
use yrs::{Any, Doc, ReadTxn, Transact, WriteTxn, XmlFragment, XmlElementPrelim};
use yrs::types::Delta;
use yrs::types::xml::{XmlDeltaPrelim, XmlIn};

use crate::db::DbConnection;
use crate::models::{NewDocumentationPage, NewDocumentationCollectionPage, DocumentationStatus};
use crate::repository;
use crate::repository::documentation_collections;

/// Run all seed checks on startup.
/// Each seed is idempotent - it only creates content if it doesn't already exist.
pub fn run_seeds(conn: &mut DbConnection) {
    seed_getting_started(conn);
}

/// Seed the "Getting Started" collection with a welcome page if it's empty.
fn seed_getting_started(conn: &mut DbConnection) {
    // Find the "Getting Started" system collection
    let collection = match documentation_collections::get_collection_by_slug(conn, "getting-started") {
        Ok(c) => c,
        Err(_) => {
            debug!("Getting Started collection not found, skipping seed");
            return;
        }
    };

    // Check if it already has pages
    match documentation_collections::get_pages_in_collection(conn, collection.id) {
        Ok(pages) if !pages.is_empty() => {
            debug!("Getting Started collection already has {} pages, skipping seed", pages.len());
            return;
        }
        Err(e) => {
            warn!(error = %e, "Failed to check Getting Started collection pages");
            return;
        }
        _ => {}
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

    // Convert markdown to a Yjs document
    let yjs_document = match markdown_to_yjs(markdown) {
        Some(doc) => doc,
        None => {
            warn!("Failed to convert getting-started.md to Yjs document");
            return;
        }
    };

    // We need a system user UUID for created_by. Use the first admin, or a nil UUID.
    let created_by = repository::users::get_users(conn)
        .ok()
        .and_then(|users| users.into_iter().find(|u| u.role == crate::models::UserRole::Admin))
        .map(|u| u.uuid)
        .unwrap_or_else(Uuid::nil);

    // Create the welcome page
    let new_page = NewDocumentationPage {
        uuid: Uuid::new_v4(),
        title: "Welcome to Nosdesk".to_string(),
        slug: "welcome".to_string(),
        icon: Some("\u{1F44B}".to_string()),
        cover_image: None,
        status: DocumentationStatus::Published,
        created_by,
        last_edited_by: created_by,
        parent_id: None,
        display_order: Some(0),
        is_public: true,
        is_template: false,
        yjs_state_vector: None,
        yjs_document: Some(yjs_document),
        yjs_client_id: None,
        has_unsaved_changes: false,
    };

    match repository::create_documentation_page(new_page, conn) {
        Ok(page) => {
            // Add it to the Getting Started collection
            let entry = NewDocumentationCollectionPage {
                collection_id: collection.id,
                page_id: page.id,
                created_by: Some(created_by),
            };
            if let Err(e) = documentation_collections::add_page_to_collection(conn, entry) {
                warn!(error = %e, "Failed to add welcome page to Getting Started collection");
            }
            info!("Seeded welcome page in Getting Started collection");
        }
        Err(e) => {
            warn!(error = %e, "Failed to create welcome page");
        }
    }
}

/// Convert a simple markdown string to a Yjs document binary (V1 encoded).
///
/// Builds a y-prosemirror compatible XmlFragment("prosemirror") with proper
/// block structure (paragraph, heading, bullet_list, etc.) and inline marks
/// (bold, italic, code) stored as text formatting attributes via XmlDeltaPrelim.
fn markdown_to_yjs(markdown: &str) -> Option<Vec<u8>> {
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
            elem.attributes.insert("level".into(), heading.level.to_string());
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
        if line.trim_start().chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && line.trim_start().contains(". ")
        {
            let mut list_items: Vec<XmlIn> = Vec::new();

            while i < lines.len() {
                let l = lines[i].trim_start();
                if l.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && l.contains(". ") {
                    let item_text = l.splitn(2, ". ").nth(1).unwrap_or("");
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
/// and [link text](url) (links rendered as plain text).
fn parse_inline_spans(text: &str) -> Vec<InlineSpan<'_>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find(|c: char| c == '*' || c == '`' || c == '[') {
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

            // Link: [text](url) -> render as plain text
            if after.starts_with('[') {
                if let Some(bracket_end) = after.find(']') {
                    let link_text = &after[1..bracket_end];
                    let after_bracket = &after[bracket_end + 1..];
                    if after_bracket.starts_with('(') {
                        if let Some(paren_end) = after_bracket.find(')') {
                            spans.push(InlineSpan::Plain(link_text));
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
