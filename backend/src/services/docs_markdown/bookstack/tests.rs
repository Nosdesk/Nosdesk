//! BookStack exports through the import: detection, the tree, bodies,
//! references, attachments, refusals, and the golden fixtures the frontend
//! checks against the editor schema.
//!
//! Fixtures under `tests/fixtures/docs_import/` are directories (a
//! `data.json` and its `files/`) zipped here: `bookstack-book`,
//! `bookstack-chapter` and `bookstack-page` are BookStack 26.09 exports,
//! scrubbed; `bookstack-no-instance` is a page export without `instance`;
//! `bookstack-edge` is written by hand.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::super::import_tests::{archive, destinations, doc, index_of, lines, nothing, outline};
use super::super::ir::{Block, Document};
use super::super::links::{fill_placeholders, Placeholder, Slot};
use super::super::profiles::ProfileKind;
use super::super::source::{DocsSource, SourceInput};
use super::super::tree::{self, Built, CollectionMode, ExcludedPages, ImportOptions, PageKind};
use super::super::NoteCode;
use super::*;
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::{ArchiveOptions, SafeArchive};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/docs_import");
const INSTANCE: &str = "00000000-0000-4000-8000-000000000001";

/// A fixture directory's files as archive entries under `prefix`.
fn fixture(name: &str, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(FIXTURES).join(name);
    let mut entries = vec![(
        format!("{prefix}data.json"),
        std::fs::read(dir.join("data.json")).expect("data.json"),
    )];
    let mut files: Vec<_> = std::fs::read_dir(dir.join("files"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        entries.push((
            format!("{prefix}files/{name}"),
            std::fs::read(&path).expect("file"),
        ));
    }
    entries
}

fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        w.start_file(name.as_str(), SimpleFileOptions::default())
            .unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn open(bytes: Vec<u8>) -> SafeArchive {
    SafeArchive::open(Cursor::new(bytes), &ArchiveOptions::documentation()).expect("opens")
}

fn entries(list: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
    list.iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect()
}

fn options() -> ImportOptions {
    ImportOptions {
        archive_name: Some("export.zip".into()),
        ..ImportOptions::default()
    }
}

fn read(name: &str) -> Built {
    read_with(name, &options())
}

fn read_with(name: &str, options: &ImportOptions) -> Built {
    read_files_with(&fixture(name, ""), options).expect("builds")
}

fn read_files_with(files: &[(String, Vec<u8>)], options: &ImportOptions) -> Result<Built, Refused> {
    tree::build(&mut archive(&entries(files)), options, &nothing())
}

fn refused(files: &[(&str, &[u8])]) -> &'static str {
    tree::build(&mut archive(files), &options(), &nothing())
        .unwrap_err()
        .code
}

fn page(index: usize) -> String {
    Placeholder::Page(index).to_href()
}

fn asset(key: &str) -> String {
    Placeholder::Asset(key.to_string()).to_href()
}

fn notes_of(built: &Built, title: &str) -> super::super::Notes {
    built.plan.pages[index_of(built, title)].notes.clone()
}

/// A page's blocks as the lines of text they hold.
fn text(built: &Built, title: &str) -> String {
    lines(doc(built, title)).join("\n")
}

// The tree.

#[test]
fn a_book_is_a_collection_in_bookstacks_order() {
    let built = read("bookstack-book");
    let plan = &built.plan;
    assert_eq!(plan.profile, ProfileKind::Bookstack);
    assert_eq!(plan.detected, None);
    assert_eq!(plan.collections.len(), 1);
    assert_eq!(plan.collections[0].name, "Help Desk Handbook");
    assert_eq!(
        plan.collections[0].description.as_deref(),
        Some("How the help desk works. See Welcome first.")
    );
    // Direct pages and chapters merged by priority; a chapter's pages under
    // it; an empty chapter kept.
    assert_eq!(
        outline(&built),
        [
            "Welcome",
            "Onboarding",
            "  VPN setup",
            "  Printer troubleshooting",
            "FAQ",
            "Empty chapter",
        ]
    );
    for page in &plan.pages {
        assert_eq!(page.collection.is_some(), page.parent.is_none(), "{page:?}");
        assert_eq!(page.source, None);
    }
    let onboarding = &plan.pages[index_of(&built, "Onboarding")];
    assert_eq!(onboarding.kind, PageKind::Section);
    assert_eq!(onboarding.key, format!("bookstack:{INSTANCE}:chapter:16"));
    assert_eq!(text(&built, "Onboarding"), "Steps for a new starter.");
    assert_eq!(
        plan.pages[index_of(&built, "VPN setup")].key,
        format!("bookstack:{INSTANCE}:page:19")
    );
    // An empty description is no content.
    assert_eq!(text(&built, "Empty chapter"), "");
    assert!(plan.pages[index_of(&built, "Empty chapter")].no_content);
    assert!(!onboarding.no_content);
    // Where a page sits reads as its titles; its id is the source's.
    let vpn = &plan.pages[index_of(&built, "VPN setup")];
    assert_eq!(vpn.display, "Onboarding / VPN setup");
    assert_eq!(vpn.source_id.as_deref(), Some("page:19"));
    assert_eq!(onboarding.display, "Onboarding");
    assert_eq!(onboarding.source_id.as_deref(), Some("chapter:16"));
    // Welcome, VPN setup and Onboarding carry tags.
    assert_eq!(plan.front_matter_not_imported.get("tags"), Some(&3));
}

#[test]
fn references_resolve_to_pages_chapters_the_book_images_and_attachments() {
    let built = read("bookstack-book");
    let welcome = index_of(&built, "Welcome");
    let onboarding = index_of(&built, "Onboarding");
    let vpn = index_of(&built, "VPN setup");
    let printer = index_of(&built, "Printer troubleshooting");
    let faq = index_of(&built, "FAQ");
    let book = Placeholder::Collection(welcome).to_href();

    // A WYSIWYG page: anchors dropped; outside the export stays a web link;
    // the image's link to itself goes.
    assert_eq!(
        destinations(doc(&built, "Welcome")),
        [
            page(vpn),
            page(onboarding),
            book.clone(),
            "https://docs.example.com/books/other-book/page/elsewhere".to_string(),
            "https://www.bookstackapp.com/docs/".to_string(),
            "mailto:help@example.com".to_string(),
            asset("files/4alZS5oUZLNyYuXvzVVK.png"),
        ]
    );
    // A Markdown page: the include links its page; a permalink resolves;
    // the link attachment is kept, and listed at the end.
    assert_eq!(
        destinations(doc(&built, "VPN setup")),
        [
            page(welcome),
            page(onboarding),
            book,
            asset("files/jOcdIz4KixevJNgoR30h.png"),
            "https://example.com/vendor".to_string(),
            page(welcome),
            page(faq),
            "https://example.com/vendor".to_string(),
        ]
    );
    let vpn_text = text(&built, "VPN setup");
    assert!(vpn_text.contains("Included from Welcome"), "{vpn_text}");
    assert!(
        vpn_text.ends_with("Attachments\nVendor portal"),
        "{vpn_text}"
    );
    let vpn_page = &built.plan.pages[vpn];
    assert_eq!(
        vpn_page.links.attachments.iter().collect::<Vec<_>>(),
        ["VPN profile guide.pdf"]
    );
    assert_eq!(vpn_page.notes.count(NoteCode::AttachmentsLeftOut), 1);
    assert_eq!(vpn_page.notes.count(NoteCode::IncludesAsLinks), 1);
    // An image another page lists; a draw.io diagram as its PNG; a file
    // attachment the body links is listed.
    assert_eq!(
        destinations(doc(&built, "Printer troubleshooting")),
        [
            asset("files/1dlVnrGIZcVOlGjNe8iq.png"),
            asset("files/4alZS5oUZLNyYuXvzVVK.png"),
        ]
    );
    let printer_page = &built.plan.pages[printer];
    assert_eq!(printer_page.notes.count(NoteCode::DiagramsAsImages), 1);
    assert_eq!(
        printer_page.links.attachments.iter().collect::<Vec<_>>(),
        ["Printer drivers.txt"]
    );
    assert_eq!(
        destinations(doc(&built, "FAQ")),
        [page(welcome), page(printer)]
    );
    for page in &built.plan.pages {
        assert!(page.links.unresolved.is_empty(), "{page:?}");
    }
}

#[test]
fn bookstack_markup_reads_as_the_editor_holds_it() {
    let built = read("bookstack-book");
    let welcome = text(&built, "Welcome");
    for line in [
        "Info",
        "Info callout text.",
        "Success",
        "Warning",
        "Danger",
        "Show more",
        "Hidden content.",
        "☑ Laptop issued",
        "☐ Badge issued",
        "sudo systemctl restart cups",
    ] {
        assert!(welcome.lines().any(|l| l == line), "{line} in {welcome}");
    }
    let notes = notes_of(&built, "Welcome");
    assert_eq!(notes.count(NoteCode::CalloutsAsQuotes), 4);
    assert_eq!(notes.count(NoteCode::ExpandsShownOpen), 1);
    assert_eq!(notes.count(NoteCode::ChecklistsAsBullets), 1);
    assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    assert_eq!(notes.count(NoteCode::HtmlRemoved), 0);
    let code = doc(&built, "Welcome").blocks.iter().find_map(|b| match b {
        Block::CodeBlock { language, .. } => language.clone(),
        _ => None,
    });
    assert_eq!(code.as_deref(), Some("bash"));
    let vpn = notes_of(&built, "VPN setup");
    assert_eq!(vpn.count(NoteCode::CalloutsAsQuotes), 1);
    assert_eq!(vpn.count(NoteCode::ChecklistsAsBullets), 1);
    let vpn_text = text(&built, "VPN setup");
    assert!(
        vpn_text.contains("Warning\nDon't share your token."),
        "{vpn_text}"
    );
}

#[test]
fn a_chapter_or_page_export_is_a_collection_named_after_it() {
    let built = read("bookstack-chapter");
    assert_eq!(built.plan.collections[0].name, "Onboarding");
    assert_eq!(
        outline(&built),
        ["Onboarding", "  VPN setup", "  Printer troubleshooting"]
    );
    // The book and its other pages aren't in it: their links stay web links.
    assert!(destinations(doc(&built, "VPN setup"))
        .contains(&"https://docs.example.com/books/help-desk-handbook/page/welcome".to_string()));
    // The image Welcome uploaded travels with the page that shows it.
    assert!(destinations(doc(&built, "Printer troubleshooting"))
        .contains(&asset("files/txd0WzarRnTEzBh3i0jG.png")));

    let built = read("bookstack-page");
    assert_eq!(built.plan.collections[0].name, "VPN setup");
    assert_eq!(outline(&built), ["VPN setup"]);
    // The included page isn't in the export: the tag stays as text.
    let vpn = text(&built, "VPN setup");
    assert!(vpn.contains("{{@15#bkmrk-getting-started}}"), "{vpn}");
    assert_eq!(
        notes_of(&built, "VPN setup").count(NoteCode::IncludesAsLinks),
        0
    );
}

#[test]
fn instance_is_optional() {
    let built = read("bookstack-no-instance");
    assert_eq!(built.plan.profile, ProfileKind::Bookstack);
    assert_eq!(built.plan.pages[0].key, "bookstack:-:page:15");
    assert_eq!(built.plan.collections[0].name, "Welcome");
}

#[test]
fn a_new_collection_mode_names_one_collection_after_the_book() {
    let built = read_with(
        "bookstack-book",
        &ImportOptions {
            collections: Some(CollectionMode::New),
            ..options()
        },
    );
    assert_eq!(built.plan.collections.len(), 1);
    assert_eq!(built.plan.collections[0].name, "Help Desk Handbook");
    assert_eq!(built.plan.collections[0].description, None);
}

// Edge cases and hostile content.

#[test]
fn edge_cases_order_key_and_title() {
    let built = read("bookstack-edge");
    // A tie puts the direct page first; same-titled siblings both stay.
    assert_eq!(
        outline(&built),
        [
            "Tie first",
            "Twins",
            "  Hostile",
            "  Twin",
            "  Twin",
            "No id",
            "Empty page",
        ]
    );
    let keys: Vec<&str> = built.plan.pages.iter().map(|p| p.key.as_str()).collect();
    assert!(
        keys.contains(&"bookstack:00000000-0000-4000-8000-000000000002:page@0.1"),
        "{keys:?}"
    );
    assert_eq!(
        built.plan.collections[0].description.as_deref(),
        Some("Edge cases for the importer.")
    );
    let empty = &built.plan.pages[index_of(&built, "Empty page")];
    assert_eq!(empty.error, None);
    // The chapter's description links into the chapter.
    assert_eq!(
        destinations(doc(&built, "Twins")),
        [page(index_of(&built, "Hostile"))]
    );
}

#[test]
fn hostile_content_keeps_only_what_a_page_may_hold() {
    let built = read("bookstack-edge");
    let row = &built.plan.pages[index_of(&built, "Hostile")];
    let body = format!("{:?}", doc(&built, "Hostile"));
    for bad in [
        "alert",
        "javascript",
        "evil",
        "onclick",
        "onerror",
        "color:red",
    ] {
        assert!(!body.contains(bad), "{bad} in {body}");
    }
    // Forged placeholders, script links and protocol-relative links keep
    // their text; a missing page is unresolved; the run book is left out.
    assert_eq!(
        destinations(doc(&built, "Hostile")),
        [
            asset("files/Vb7sK2mQ9wLx4ZcR1hTa.svg"),
            "https://example.com/spec.pdf".to_string(),
        ]
    );
    assert_eq!(row.notes.count(NoteCode::LinksDropped), 3);
    assert_eq!(row.notes.count(NoteCode::LinksUnresolved), 1);
    assert_eq!(row.notes.count(NoteCode::AttachmentsLeftOut), 2);
    assert_eq!(
        row.links.attachments.iter().collect::<Vec<_>>(),
        ["Run book.docx", "Unlinked notes.txt"]
    );
    assert_eq!(
        row.links.missing_images.iter().collect::<Vec<_>>(),
        ["bsexport://0/image/62"]
    );
    let text = text(&built, "Hostile");
    assert!(text.contains("Note\nAn unknown callout type."), "{text}");
    assert!(text.contains("Text include {{@40}} stays text."), "{text}");
    assert!(text.contains("☑ Checked by its item"), "{text}");
    // Only the web link attachment is listed; the script one isn't.
    assert!(text.ends_with("Attachments\nSpec"), "{text}");
}

#[test]
fn markdown_pages_read_bookstack_html_blocks_and_includes() {
    let built = read("bookstack-edge");
    let twin = index_of(&built, "Twin");
    let row = &built.plan.pages[twin];
    let text = text(&built, "Twin");
    assert!(text.contains("Included from Hostile"), "{text}");
    assert!(text.contains("missing: {{@999}}"), "{text}");
    assert!(text.contains("Note\nA callout with no type."), "{text}");
    assert!(text.contains("Open me\nHidden text."), "{text}");
    assert_eq!(row.notes.count(NoteCode::IncludesAsLinks), 1);
    assert_eq!(row.notes.count(NoteCode::ExpandsShownOpen), 1);
    assert_eq!(row.notes.count(NoteCode::DiagramsAsImages), 1);
    assert_eq!(row.notes.count(NoteCode::CalloutsAsQuotes), 1);
    // The book link goes to its collection; a link to an image is text.
    assert_eq!(
        destinations(doc(&built, "Twin")),
        [
            page(index_of(&built, "Hostile")),
            asset("files/Tn3QeA8bW2xYp5LcK9dR.png"),
            Placeholder::Collection(index_of(&built, "Tie first")).to_href(),
        ]
    );
    assert_eq!(
        row.links.unresolved.iter().collect::<Vec<_>>(),
        ["bsexport://0/image/61"]
    );
}

#[test]
fn an_unticked_page_goes_by_its_key_not_its_title() {
    let first = read("bookstack-edge");
    let twins: Vec<&tree::PlannedPage> = first
        .plan
        .pages
        .iter()
        .filter(|p| p.title == "Twin")
        .collect();
    let mut exclude = ExcludedPages::default();
    exclude.add(None, None, &twins[1].key, &["Twins".into(), "Twin".into()]);
    assert!(exclude.trails.is_empty());
    let built = read_with(
        "bookstack-edge",
        &ImportOptions {
            exclude,
            ..options()
        },
    );
    let kept: Vec<&str> = built
        .plan
        .pages
        .iter()
        .filter(|p| p.title == "Twin")
        .map(|p| p.key.as_str())
        .collect();
    assert_eq!(kept, [twins[0].key.as_str()]);
}

// Bundles, detection and refusals.

#[test]
fn a_zip_of_exports_is_a_collection_each() {
    let book = zip(&fixture("bookstack-book", ""));
    let chapter = zip(&fixture("bookstack-chapter", ""));
    let outer = zip(&[
        ("Help Desk Handbook.zip".to_string(), book),
        ("Onboarding.zip".to_string(), chapter),
    ]);
    let built = tree::build(&mut open(outer), &options(), &nothing()).expect("builds");
    let names: Vec<&str> = built
        .plan
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Help Desk Handbook", "Onboarding"]);
    assert_eq!(built.plan.pages.len(), 9);
    // The same page in both exports: the second key is told apart.
    let keys: Vec<&str> = built
        .plan
        .pages
        .iter()
        .filter(|p| p.title == "VPN setup")
        .map(|p| p.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            format!("bookstack:{INSTANCE}:page:19"),
            format!("bookstack:{INSTANCE}:page:19#2")
        ]
    );
    // Each export's references stay inside it.
    let second_printer = built
        .plan
        .pages
        .iter()
        .rposition(|p| p.title == "Printer troubleshooting")
        .unwrap();
    assert_eq!(
        destinations(&built.documents[second_printer]),
        [
            asset("Onboarding/files/rdXZc11AzvC31lVOEWZw.png"),
            asset("Onboarding/files/txd0WzarRnTEzBh3i0jG.png"),
        ]
    );
}

#[test]
fn a_wrapper_folder_and_the_compact_json_bookstack_writes_are_read() {
    let data = r#"{"page":{"id":3,"name":"A\/B","html":"<p>One<\/p>","priority":1,"attachments":[],"images":[],"tags":[]},"exported_at":"2026-11-09T00:00:00+00:00","instance":{"id":"x","version":"v26.09.2"}}"#;
    let built = tree::build(
        &mut archive(&[
            ("export/data.json", data.as_bytes()),
            ("export/files/a.txt", b"a"),
        ]),
        &options(),
        &nothing(),
    )
    .expect("builds");
    assert_eq!(built.plan.pages[0].title, "A/B");
    assert_eq!(built.plan.root, "export");
    assert_eq!(lines(&built.documents[0]), ["One"]);
}

#[test]
fn a_data_json_that_isnt_bookstacks_is_left_to_the_markdown_profiles() {
    let built = tree::build(
        &mut archive(&[
            ("data.json", br#"{"rows": [1, 2]}"#),
            ("guide.md", b"# Guide\n\nText.\n"),
        ]),
        &options(),
        &nothing(),
    )
    .expect("builds");
    assert_eq!(built.plan.profile, ProfileKind::Plain);
    assert_eq!(outline(&built), ["Guide"]);
}

#[test]
fn a_forced_profile_says_bookstack_was_detected() {
    let mut files = fixture("bookstack-page", "");
    files.push(("files/notes.md".into(), b"# Notes\n".to_vec()));
    let built = read_files_with(
        &files,
        &ImportOptions {
            profile: Some(ProfileKind::Plain),
            ..options()
        },
    )
    .expect("builds");
    assert_eq!(built.plan.profile, ProfileKind::Plain);
    assert_eq!(built.plan.detected, Some(ProfileKind::Bookstack));

    let err = read_files_with(
        &[("a.md".to_string(), b"# A".to_vec())],
        &ImportOptions {
            profile: Some(ProfileKind::Bookstack),
            ..options()
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "docs_import_bookstack_unreadable");
}

#[test]
fn exports_this_version_cant_read_are_refused() {
    assert_eq!(
        refused(&[("data.json", b"{\"book\": "), ("files/a.png", b"png")]),
        "docs_import_bookstack_unreadable"
    );
    assert_eq!(
        refused(&[("data.json", br#"{"book": {"name": 3}}"#)]),
        "docs_import_bookstack_unreadable"
    );
    assert_eq!(
        refused(&[(
            "data.json",
            br#"{"books": [{"name": "A"}], "instance": {"id": "x"}}"#
        )]),
        "docs_import_bookstack_unsupported"
    );
    assert_eq!(
        refused(&[(
            "data.json",
            br#"{"book": {"name": "A"}, "page": {"name": "B"}}"#
        )]),
        "docs_import_bookstack_unsupported"
    );
}

/// A zip of uncompressed entries, so the ratio bound doesn't refuse a
/// large repetitive one first.
fn stored(files: &[(&str, &[u8])]) -> SafeArchive {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    for (name, bytes) in files {
        w.start_file(*name, stored).unwrap();
        w.write_all(bytes).unwrap();
    }
    open(w.finish().unwrap().into_inner())
}

#[test]
fn a_data_json_over_its_cap_is_refused_with_the_chapter_advice() {
    let big = vec![b' '; MAX_DATA_JSON_BYTES as usize + 1];
    let mut archive = stored(&[("data.json", &big), ("files/a.png", b"png")]);
    let err = tree::build(&mut archive, &options(), &nothing()).unwrap_err();
    assert_eq!(err.code, "docs_import_bookstack_too_large");
    assert_eq!(
        err.message,
        "This BookStack export is larger than we can read at once. Export one chapter at a time."
    );
}

#[test]
fn a_page_over_two_megabytes_is_imported_empty_with_its_error() {
    let html = format!("<p>{}</p>", "x".repeat(MAX_MARKDOWN_BYTES as usize));
    let data = serde_json::json!({ "page": { "id": 1, "name": "Big", "html": html } }).to_string();
    let built = tree::build(
        &mut stored(&[("data.json", data.as_bytes())]),
        &options(),
        &nothing(),
    )
    .expect("builds");
    assert_eq!(
        built.plan.pages[0].error.as_deref(),
        Some("This page's content is larger than 2 MB.")
    );
    assert_eq!(built.documents[0], Document::default());
}

// References.

#[test]
fn references_rewrite_only_in_bookstacks_own_form() {
    assert_eq!(
        rewrite_refs(
            "[x]([[bsexport:page:12]]#bkmrk-a) ![]([[bsexport:image:3]])",
            2
        ),
        "[x](bsexport://2/page/12#bkmrk-a) ![](bsexport://2/image/3)"
    );
    for kept in [
        "[[bsexport:Page:1]]",
        "[[bsexport:page:]]",
        "[[bsexport:page:1]",
        "[[bsexport:page:x]]",
        "[[bsexport::1]]",
    ] {
        assert_eq!(rewrite_refs(kept, 0), kept);
    }
    assert_eq!(
        rewrite_refs("[[bsexport:[[bsexport:book:1]]", 0),
        "[[bsexport:bsexport://0/book/1"
    );
}

#[test]
fn another_source_cant_use_the_bookstack_scheme() {
    let built = tree::build(
        &mut archive(&[(
            "a.md",
            b"[x](bsexport://0/page/1) ![y](bsexport://0/image/1)\n",
        )]),
        &options(),
        &nothing(),
    )
    .expect("builds");
    assert!(destinations(&built.documents[0]).is_empty());
    assert_eq!(built.plan.pages[0].notes.count(NoteCode::LinksDropped), 1);
    assert_eq!(built.plan.pages[0].notes.count(NoteCode::ImagesDropped), 1);
}

#[test]
fn the_source_registers_its_own_destinations() {
    let files = fixture("bookstack-page", "");
    let mut archive = archive(&entries(&files));
    let keys: Vec<String> = archive.entries().iter().map(|e| e.key.clone()).collect();
    let exports = Exports::find(&mut archive, &keys).unwrap().expect("found");
    let mut source = BookstackSource::new(exports);
    let tree = source
        .read(&mut SourceInput::Archive {
            archive: &mut archive,
            files: &keys,
        })
        .expect("reads");
    assert_eq!(tree.pages.len(), 1);
    assert_eq!(tree.refs.schemes, [SCHEME]);
    assert!(tree.refs.targets.contains_key("bsexport://0/image/7"));
    assert!(tree.refs.targets.contains_key("bsexport://0/page/19"));
}

// Golden fixtures: each page of the book and of the edge cases, its
// placeholders filled as a commit would, checked against the editor schema
// by `docsMarkdownFixtures.spec.ts`.

#[test]
fn golden_fixtures_are_current() {
    let regenerate = std::env::var_os("UPDATE_DOCS_MARKDOWN_FIXTURES").is_some();
    let mut stale = Vec::new();
    for name in ["bookstack-book", "bookstack-edge"] {
        let built = read(name);
        for (page, document) in built.plan.pages.iter().zip(&built.documents) {
            let mut document = document.clone();
            let fill = |p: &Placeholder, _: Slot| -> Option<String> {
                Some(match p {
                    Placeholder::Page(i) => format!("/documentation/{}", i + 1),
                    Placeholder::Collection(_) => "/documentation/collections/imported".into(),
                    Placeholder::Asset(key) => format!(
                        "/api/files/collab/doc/00000000-0000-4000-8000-000000000000/{}",
                        key.rsplit('/').next().unwrap_or(key)
                    ),
                })
            };
            fill_placeholders(&mut document, &fill);
            let slug: String = page
                .title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let fixture = format!("{name}.{:02}-{slug}", page.index);
            if !super::super::tests::golden_is_current(
                &fixture,
                &fixture,
                &document,
                &page.notes,
                regenerate,
            ) {
                stale.push(fixture);
            }
        }
    }
    assert!(
        stale.is_empty(),
        "stale BookStack golden fixtures {stale:?}; run UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown"
    );
}
