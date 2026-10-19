//! The import path end to end: a small synthetic archive per source, read
//! into a plan and documents.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::ir::{blocks_text, Block, Document, Inline};
use super::links::{Placeholder, Workspace};
use super::profiles::ProfileKind;
use super::tree::{self, Built, CollectionMode, ImportOptions, PageStatus, StatusMode};
use super::NoteCode;
use crate::services::uploaded_archive::{ArchiveOptions, SafeArchive};

/// An archive of `(name, contents)` entries, opened as the import opens one.
pub(crate) fn archive(files: &[(&str, &[u8])]) -> SafeArchive {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(bytes).unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    SafeArchive::open(Cursor::new(bytes), &ArchiveOptions::documentation()).expect("opens")
}

/// A workspace holding the given users, tickets, pages and images, that
/// exported archives carry the uuid [`WORKSPACE`].
pub(crate) struct Here {
    pub users: Vec<&'static str>,
    pub tickets: Vec<&'static str>,
    pub pages: Vec<&'static str>,
    pub images: Vec<&'static str>,
}

pub(crate) const WORKSPACE: &str = "11111111-1111-4111-8111-111111111111";

impl Workspace for Here {
    fn is_this_workspace(&self, uuid: &str) -> bool {
        uuid == WORKSPACE
    }
    fn user_exists(&self, uuid: &str) -> bool {
        self.users.contains(&uuid)
    }
    fn ticket_exists(&self, id: &str) -> bool {
        self.tickets.contains(&id)
    }
    fn page_exists(&self, uuid: &str) -> bool {
        self.pages.contains(&uuid)
    }
    fn image_exists(&self, src: &str) -> bool {
        self.images.contains(&src)
    }
}

fn nothing() -> Here {
    Here {
        users: vec![],
        tickets: vec![],
        pages: vec![],
        images: vec![],
    }
}

fn text<'a>(files: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a [u8])> {
    files.iter().map(|(n, t)| (*n, t.as_bytes())).collect()
}

fn build(files: &[(&str, &[u8])], options: &ImportOptions, workspace: &dyn Workspace) -> Built {
    tree::build(&mut archive(files), options, workspace).expect("builds")
}

fn options() -> ImportOptions {
    ImportOptions {
        archive_name: Some("IT-Handbook.zip".into()),
        ..ImportOptions::default()
    }
}

/// The tree as indented titles, in plan order.
fn outline(built: &Built) -> Vec<String> {
    built
        .plan
        .pages
        .iter()
        .map(|page| {
            let mut depth = 0;
            let mut parent = page.parent;
            while let Some(p) = parent {
                depth += 1;
                parent = built.plan.pages[p].parent;
            }
            format!("{}{}", "  ".repeat(depth), page.title)
        })
        .collect()
}

fn index_of(built: &Built, title: &str) -> usize {
    built
        .plan
        .pages
        .iter()
        .position(|p| p.title == title)
        .unwrap_or_else(|| panic!("no page {title}: {:?}", outline(built)))
}

fn doc<'a>(built: &'a Built, title: &str) -> &'a Document {
    &built.documents[index_of(built, title)]
}

fn page_href(built: &Built, title: &str) -> String {
    Placeholder::Page(index_of(built, title)).to_href()
}

/// Every link href, image src and embedded page uuid in a document, in
/// order; a link split across runs once.
fn destinations(document: &Document) -> Vec<String> {
    fn inlines(list: &[Inline], out: &mut Vec<String>) {
        let mut last: Option<String> = None;
        for inline in list {
            match inline {
                Inline::Text(t) => {
                    let href = t.marks.link.as_ref().map(|l| l.href.clone());
                    if let Some(h) = href.as_ref().filter(|_| href != last) {
                        out.push(h.clone());
                    }
                    last = href;
                }
                Inline::Image(image) => {
                    out.push(image.src.clone());
                    last = None;
                }
                _ => last = None,
            }
        }
    }
    fn blocks(list: &[Block], out: &mut Vec<String>) {
        for block in list {
            match block {
                Block::Paragraph(i) | Block::Heading { content: i, .. } => inlines(i, out),
                Block::Blockquote(inner) => blocks(inner, out),
                Block::BulletList(items) | Block::OrderedList { items, .. } => {
                    for item in items {
                        blocks(&item.blocks, out);
                    }
                }
                Block::Table(table) => {
                    for cell in table.rows.iter().flatten() {
                        blocks(&cell.blocks, out);
                    }
                }
                Block::EmbeddedDocument { document_uuid, .. } => out.push(document_uuid.clone()),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    blocks(&document.blocks, &mut out);
    out
}

fn lines(document: &Document) -> Vec<String> {
    blocks_text(&document.blocks)
}

fn asset(key: &str) -> String {
    Placeholder::Asset(key.to_string()).to_href()
}

// Plain folders and loose files.

#[test]
fn a_plain_folder_orders_titles_and_resolves_relative_links() {
    let files = text(&[
        ("handbook/index.md", "# Welcome\n\nHome page."),
        (
            "handbook/01-getting-started.md",
            "# Getting started\n\nSee [VPN](02-network/vpn-setup.md), [Wi-Fi](./02-network/wifi), \
             [the network](02-network/), [Printers](../handbook/printers.md#top), \
             [remote](02-network/Remote%20access.md) and [missing](nowhere.md).\n\n\
             ![Diagram](images/diagram.png) ![Gone](images/gone.png)\n\n[A PDF](files/policy.pdf)\n",
        ),
        (
            "handbook/02-network/index.md",
            "---\ntitle: Network\n---\nNetwork overview.",
        ),
        ("handbook/02-network/vpn-setup.md", "---\nweight: 2\n---\nVPN body"),
        (
            "handbook/02-network/wifi.md",
            "---\nweight: 1\ntags: [a]\nauthor: Sam\n---\n# Wi-Fi\n\nBody",
        ),
        ("handbook/02-network/Remote access.md", "Remote."),
        ("handbook/printers.md", "# Printers\n"),
        ("handbook/policies/security/passwords.md", "Use a manager."),
        ("handbook/images/diagram.png", "png"),
        ("handbook/files/policy.pdf", "pdf"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Plain);
    assert_eq!(
        outline(&built),
        vec![
            "Welcome",
            "Getting started",
            "Network",
            "  Wi-Fi",
            "  Vpn setup",
            "  Remote access",
            "Policies",
            "  Security",
            "    Passwords",
            "Printers",
        ]
    );
    assert_eq!(built.plan.section_pages, 2);
    assert_eq!(built.plan.collections.len(), 1);
    assert_eq!(built.plan.collections[0].name, "IT-Handbook");
    assert!(built
        .plan
        .pages
        .iter()
        .all(|p| p.status == PageStatus::Draft));
    assert_eq!(
        built.plan.front_matter_not_imported,
        [("author".to_string(), 1), ("tags".to_string(), 1)].into()
    );

    let start = doc(&built, "Getting started");
    assert_eq!(
        destinations(start),
        vec![
            page_href(&built, "Vpn setup"),
            page_href(&built, "Wi-Fi"),
            page_href(&built, "Network"),
            page_href(&built, "Printers"),
            page_href(&built, "Remote access"),
            asset("handbook/images/diagram.png"),
        ]
    );
    // The opening H1 repeated the title.
    assert!(!matches!(start.blocks[0], Block::Heading { .. }));
    let page = &built.plan.pages[index_of(&built, "Getting started")];
    assert_eq!(page.links.rewritten, 5);
    assert_eq!(
        page.links.unresolved.iter().collect::<Vec<_>>(),
        vec!["nowhere.md"]
    );
    assert!(page.links.missing_images.contains("images/gone.png"));
    assert!(page.links.attachments.contains("files/policy.pdf"));
    assert_eq!(page.notes.count(NoteCode::LinksUnresolved), 1);
    assert_eq!(page.notes.count(NoteCode::AttachmentsLeftOut), 1);
    assert_eq!(page.notes.count(NoteCode::ImagesDropped), 1);
    assert_eq!(page.notes.count(NoteCode::LinksDropped), 0);
    assert!(lines(start).iter().any(|l| l.contains("missing")));
}

#[test]
fn loose_files_are_top_level_pages() {
    let files = text(&[
        ("b-side.md", "Second"),
        ("a_side.md", "First"),
        ("2. Two.md", "Two"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(outline(&built), vec!["Two", "A side", "B side"]);
}

#[test]
fn leave_out_globs_and_status_options() {
    let files = text(&[
        ("docs/a.md", "---\ndraft: true\n---\nA"),
        ("docs/b.md", "---\npublished: false\n---\nB"),
        ("docs/c.md", "C"),
        ("docs/archive/old.md", "Old"),
    ]);
    let built = build(
        &files,
        &ImportOptions {
            leave_out: vec!["**/archive/**".into()],
            status: StatusMode::AsFiles,
            ..options()
        },
        &nothing(),
    );
    assert_eq!(outline(&built), vec!["A", "B", "C"]);
    let statuses: Vec<PageStatus> = built.plan.pages.iter().map(|p| p.status).collect();
    assert_eq!(
        statuses,
        vec![PageStatus::Draft, PageStatus::Draft, PageStatus::Published]
    );
    assert_eq!(built.plan.left_out.len(), 1);
}

// Nosdesk exports.

fn manifest() -> String {
    format!(
        r#"{{
  "format": "nosdesk-docs",
  "format_version": 2,
  "workspace_uuid": "{WORKSPACE}",
  "exported_at": "2026-10-01T00:00:00Z",
  "collections": [
    {{ "uuid": "c2", "name": "Second", "slug": "second", "path": "second", "order": 2 }},
    {{ "uuid": "c1", "name": "IT", "slug": "it", "path": "it", "order": 1, "icon": "🖥️" }}
  ],
  "pages": [
    {{ "uuid": "p1", "title": "VPN", "slug": "vpn", "path": "it/vpn.md", "parent": null, "collection": "c1", "order": 2, "status": "published", "icon": "🔐" }},
    {{ "uuid": "p2", "title": "Wi-Fi", "slug": "wifi", "path": "it/wifi.md", "parent": null, "collection": "c1", "order": 1, "status": "draft", "icon": null }},
    {{ "uuid": "p3", "title": "Client setup", "slug": "client-setup", "path": "it/vpn/client-setup.md", "parent": "p1", "collection": null, "order": null, "status": "archived", "icon": null }},
    {{ "uuid": "p4", "title": "Old", "slug": "old", "path": "second/old.md", "parent": null, "collection": "c2", "order": 1, "status": "deleted", "icon": null }},
    {{ "uuid": "p5", "title": "Kept child", "slug": "kept", "path": "second/old/kept.md", "parent": "p4", "collection": null, "order": 1, "status": "published", "icon": null }}
  ]
}}"#
    )
}

const V2_VPN: &str = "---\ntitle: \"VPN\"\nuuid: p1\nslug: \"vpn\"\nstatus: published\nicon: \"🔐\"\n---\n\n\
# VPN\n\nAsk <span data-mention=\"true\" data-uuid=\"u-1\" data-name=\"Ana\">@Ana</span> about \
<span data-ticket-link=\"true\" data-ticket-id=\"42\" data-href=\"/tickets/42\">Ticket #7</span>, \
see [Wi-Fi](wifi.md).\n\n\
<div data-embedded-document=\"true\" data-document-uuid=\"p3\" data-document-title=\"Client setup\">Client setup</div>\n\n\
<div data-embedded-document=\"true\" data-document-uuid=\"elsewhere\" data-document-title=\"Elsewhere\">Elsewhere</div>\n\n\
<img src=\"../_assets/p1/1_a.png\" alt=\"A\" width=\"240\" data-align=\"center\">\n";

fn build_v2(workspace: &dyn Workspace, status: StatusMode) -> Built {
    let manifest = manifest();
    let files: Vec<(&str, &[u8])> = vec![
        ("nosdesk-docs.json", manifest.as_bytes()),
        ("it/vpn.md", V2_VPN.as_bytes()),
        ("it/wifi.md", b"---\ntitle: \"Wi-Fi\"\n---\n\nWireless.\n"),
        (
            "it/vpn/client-setup.md",
            b"---\ntitle: \"Client setup\"\n---\n\nInstall.\n",
        ),
        ("second/old.md", b"Old"),
        ("second/old/kept.md", b"Kept"),
        ("_assets/p1/1_a.png", b"png"),
    ];
    build(
        &files,
        &ImportOptions {
            status,
            ..options()
        },
        workspace,
    )
}

#[test]
fn a_nosdesk_export_keeps_its_tree_collections_and_statuses() {
    let here = Here {
        users: vec!["u-1"],
        tickets: vec!["42"],
        pages: vec!["elsewhere"],
        images: vec![],
    };
    let built = build_v2(&here, StatusMode::AsFiles);
    assert_eq!(built.plan.profile, ProfileKind::NosdeskV2);
    assert_eq!(
        outline(&built),
        vec!["Wi-Fi", "VPN", "  Client setup", "Kept child"]
    );
    let names: Vec<&str> = built
        .plan
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["IT", "Second"]);
    let vpn = &built.plan.pages[index_of(&built, "VPN")];
    assert_eq!(vpn.collection, Some(0));
    assert_eq!(vpn.order, 1);
    assert_eq!(vpn.icon.as_deref(), Some("🔐"));
    assert_eq!(vpn.uuid.as_deref(), Some("p1"));
    assert_eq!(vpn.status, PageStatus::Published);
    assert_eq!(
        built.plan.pages[index_of(&built, "Client setup")].status,
        PageStatus::Archived
    );
    assert_eq!(
        built.plan.pages[index_of(&built, "Wi-Fi")].status,
        PageStatus::Draft
    );
    assert!(built
        .plan
        .left_out
        .iter()
        .any(|l| l.path == "second/old.md" && l.reason == "deleted"));

    let body = doc(&built, "VPN");
    // An export's opening heading is content, not the title repeated.
    assert!(matches!(&body.blocks[0], Block::Heading { level: 1, .. }));
    let Block::Paragraph(inlines) = &body.blocks[1] else {
        panic!("{:?}", body.blocks[1]);
    };
    assert!(inlines
        .iter()
        .any(|i| matches!(i, Inline::Mention { uuid, .. } if uuid == "u-1")));
    assert!(inlines
        .iter()
        .any(|i| matches!(i, Inline::TicketLink { ticket_id, .. } if ticket_id == "42")));
    assert_eq!(
        destinations(body),
        vec![
            page_href(&built, "Wi-Fi"),
            page_href(&built, "Client setup"),
            "elsewhere".to_string(),
            asset("_assets/p1/1_a.png"),
        ]
    );
    let Some(Block::Paragraph(last)) = body.blocks.last() else {
        panic!("an image paragraph");
    };
    assert!(matches!(&last[0], Inline::Image(img) if img.width == Some(240)));
}

#[test]
fn spans_from_another_workspace_become_text() {
    let built = build_v2(&nothing(), StatusMode::Drafts);
    let body = doc(&built, "VPN");
    let text = lines(body).join("\n");
    assert!(
        text.contains("Ask @Ana about Ticket #7, see Wi-Fi."),
        "{text}"
    );
    // The embedded page is in the import, so it is still embedded; the one
    // outside it is its title.
    assert!(body.blocks.iter().any(|b| matches!(
        b,
        Block::EmbeddedDocument { document_title, .. } if document_title == "Client setup"
    )));
    assert!(text.contains("Elsewhere"));
    let page = &built.plan.pages[index_of(&built, "VPN")];
    assert_eq!(page.notes.count(NoteCode::MentionsAsText), 1);
    assert_eq!(page.notes.count(NoteCode::TicketLinksAsText), 1);
    assert_eq!(page.notes.count(NoteCode::EmbedsAsText), 1);
    assert!(built
        .plan
        .pages
        .iter()
        .all(|p| p.status == PageStatus::Draft));
}

#[test]
fn a_nosdesk_v1_export_resolves_slug_links() {
    let files = text(&[
        (
            "documentation/vpn-setup.md",
            "---\ntitle: VPN setup\nslug: vpn-setup\nstatus: published\nicon: 🔐\ncreated: 2025-01-01\n---\n# VPN setup\n\nSee [Wi-Fi](/documentation/wifi) and [gone](/documentation/gone).\n",
        ),
        (
            "documentation/wifi/index.md",
            "---\ntitle: Wi-Fi\nslug: wifi\nstatus: draft\n---\n# Wi-Fi\n\n![x](/api/files/collab/doc/abc/1_x.png) ![y](/api/files/collab/doc/abc/2_y.png)\n",
        ),
        (
            "documentation/wifi/guest.md",
            "---\ntitle: Guest network\nslug: guest-network\nstatus: published\n---\n# Guest network\n",
        ),
    ]);
    let here = Here {
        images: vec!["/api/files/collab/doc/abc/1_x.png"],
        ..nothing()
    };
    let built = build(&files, &options(), &here);
    assert_eq!(built.plan.profile, ProfileKind::NosdeskV1);
    assert_eq!(
        outline(&built),
        vec!["VPN setup", "Wi-Fi", "  Guest network"]
    );
    let vpn = doc(&built, "VPN setup");
    assert_eq!(destinations(vpn), vec![page_href(&built, "Wi-Fi")]);
    assert_eq!(lines(vpn), vec!["See Wi-Fi and gone."]);
    assert_eq!(
        built.plan.pages[0].slug.as_deref(),
        Some("vpn-setup"),
        "the slug is kept for the commit to reuse"
    );
    assert_eq!(built.plan.pages[0].icon.as_deref(), Some("🔐"));
    assert_eq!(
        destinations(doc(&built, "Wi-Fi")),
        vec!["/api/files/collab/doc/abc/1_x.png".to_string()]
    );
}

// Notion.

const WIKI: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn a_notion_export_strips_ids_and_orders_children_by_link() {
    let wiki_md = format!("Wiki {WIKI}.md");
    let zebra = format!("Wiki {WIKI}/Zebra aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md");
    let apple = format!("Wiki {WIKI}/Apple bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.md");
    let csv = format!("Wiki {WIKI}/Tasks cccccccccccccccccccccccccccccccc.csv");
    let csv_all = format!("Wiki {WIKI}/Tasks cccccccccccccccccccccccccccccccc_all.csv");
    let row = format!(
        "Wiki {WIKI}/Tasks cccccccccccccccccccccccccccccccc/One dddddddddddddddddddddddddddddddd.md"
    );
    let wiki_body = format!(
        "# Wiki\n\n[Zebra](Wiki%20{WIKI}/Zebra%20aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md)\n\n\
         [Apple](Wiki%20{WIKI}/Apple%20bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.md)\n\n\
         <aside>\n💡 Restart first.\n\nThen call **IT**.\n\n</aside>\n\n\
         [Apple on Notion](https://www.notion.so/team/Apple-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb?pvs=4)\n"
    );
    let files: Vec<(&str, &[u8])> = vec![
        (&wiki_md, wiki_body.as_bytes()),
        (&zebra, b"# Zebra\n\nZ"),
        (&apple, b"# Apple\n\nA"),
        (&csv, b"Name,Status\nOne,Done\n"),
        (&csv_all, b"Name,Status\nOne,Done\nTwo,Open\n"),
        (&row, b"# One\n\nStatus: Done"),
    ];
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Notion);
    assert_eq!(
        outline(&built),
        vec!["Wiki", "  Zebra", "  Apple", "  Tasks", "    One"]
    );
    assert_eq!(built.plan.collections[0].name, "Wiki");
    let wiki = doc(&built, "Wiki");
    assert_eq!(
        destinations(wiki),
        vec![
            page_href(&built, "Zebra"),
            page_href(&built, "Apple"),
            page_href(&built, "Apple"),
        ]
    );
    let quote = wiki
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Blockquote(inner) => Some(inner),
            _ => None,
        })
        .expect("the aside is a callout");
    assert_eq!(
        blocks_text(quote),
        vec!["💡", "Restart first.", "Then call IT."]
    );
    let tasks = doc(&built, "Tasks");
    let Block::Table(table) = &tasks.blocks[0] else {
        panic!("the database is a table: {tasks:?}");
    };
    assert_eq!(table.rows.len(), 3, "the _all export, with every row");
    assert!(table.rows[0][0].header);
}

// Obsidian.

#[test]
fn an_obsidian_vault_resolves_wikilinks_embeds_and_aliases() {
    let files = text(&[
        (".obsidian/app.json", "{}"),
        (
            "Home.md",
            "Links: [[Setup]], [[Guides/Setup|the guide]], [[Printer#Drivers]], [[Printing]], [[VPN alias]], [[Nowhere]].\n\n\
             ![[diagram.png|300]]\n\n![[Printer]]\n\n%% private %%\nAfter\n",
        ),
        ("Setup.md", "Top setup"),
        ("Guides/Setup.md", "Guide setup"),
        ("Guides/Printer.md", "---\naliases: [Printing]\n---\nPrinter body"),
        ("Guides/Guides.md", "Folder note"),
        ("Network/VPN.md", "---\naliases:\n  - VPN alias\n---\nVPN"),
        ("attachments/diagram.png", "png"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Obsidian);
    assert_eq!(
        outline(&built),
        vec![
            "Guides",
            "  Printer",
            "  Setup",
            "Home",
            "Network",
            "  VPN",
            "Setup",
        ]
    );
    let home = doc(&built, "Home");
    let top_setup = built
        .plan
        .pages
        .iter()
        .position(|p| p.title == "Setup" && p.parent.is_none())
        .unwrap();
    let guide_setup = built
        .plan
        .pages
        .iter()
        .position(|p| p.title == "Setup" && p.parent.is_some())
        .unwrap();
    assert_eq!(
        destinations(home),
        vec![
            Placeholder::Page(top_setup).to_href(),
            Placeholder::Page(guide_setup).to_href(),
            page_href(&built, "Printer"),
            page_href(&built, "Printer"),
            page_href(&built, "VPN"),
            asset("attachments/diagram.png"),
            page_href(&built, "Printer"),
        ]
    );
    let first = &lines(home)[0];
    assert!(
        first
            .starts_with("Links: Setup, the guide, Printer#Drivers, Printing, VPN alias, Nowhere."),
        "{first}"
    );
    let image = home
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Paragraph(i) => i.iter().find_map(|i| match i {
                Inline::Image(img) => Some(img.clone()),
                _ => None,
            }),
            _ => None,
        })
        .unwrap();
    assert_eq!(image.width, Some(300));
    assert!(home.blocks.iter().any(|b| matches!(
        b,
        Block::EmbeddedDocument { document_title, .. } if document_title == "Printer"
    )));
    assert!(!lines(home).iter().any(|l| l.contains("private")));
    let report = &built.plan.pages[index_of(&built, "Home")].links;
    assert_eq!(
        report.ambiguous.get("Setup"),
        Some(&vec![top_setup, guide_setup])
    );
    assert!(report.unresolved.contains("[[Nowhere]]"));
    let notes = &built.plan.pages[index_of(&built, "Home")].notes;
    assert_eq!(notes.count(NoteCode::LinksAmbiguous), 1);
    assert_eq!(notes.count(NoteCode::LinksUnresolved), 1);
    assert_eq!(notes.count(NoteCode::SourceSyntaxRemoved), 1);
}

// Site generators.

#[test]
fn a_gitbook_repo_follows_its_summary() {
    let files = text(&[
        (".gitbook.yaml", "root: ./docs/\n"),
        ("docs/README.md", "# Welcome\n\nHello"),
        (
            "docs/SUMMARY.md",
            "# Table of contents\n\n* [Welcome](README.md)\n\n## Guides\n\n* [Setup guide](guides/setup.md)\n  * [VPN](guides/vpn.md)\n* [Printers](guides/printers.md)\n",
        ),
        (
            "docs/guides/setup.md",
            "{% hint style=\"warning\" %}\nBack up.\n{% endhint %}\n\n![Diagram](../.gitbook/assets/diagram.png)\n\n{% embed url=\"https://e.example/video\" %}\n",
        ),
        ("docs/guides/vpn.md", "VPN"),
        ("docs/guides/printers.md", "# Printing\n\nPrinters"),
        ("docs/guides/unlisted.md", "Extra"),
        ("docs/.gitbook/assets/diagram.png", "png"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Gitbook);
    assert_eq!(
        outline(&built),
        vec![
            "Welcome",
            "Guides",
            "  Setup guide",
            "    VPN",
            "  Printing",
            "Unlisted",
        ]
    );
    let setup = doc(&built, "Setup guide");
    let Block::Blockquote(inner) = &setup.blocks[0] else {
        panic!("a hint is a callout: {setup:?}");
    };
    assert_eq!(blocks_text(inner), vec!["Warning", "Back up."]);
    assert_eq!(
        destinations(setup),
        vec![
            asset("docs/.gitbook/assets/diagram.png"),
            "https://e.example/video".to_string()
        ]
    );
}

#[test]
fn mkdocs_follows_its_nav_and_names_the_collection() {
    let files = text(&[
        (
            "mkdocs.yml",
            "site_name: IT Handbook\ntheme:\n  name: material\nmarkdown_extensions:\n  - pymdownx.emoji:\n      emoji_index: !!python/name:material.extensions.emoji.twemoji\nnav:\n  - Home: index.md\n  - Guides:\n    - Setup: guides/setup.md\n    - guides/vpn.md\n  - External: https://example.com\n",
        ),
        ("docs/index.md", "Welcome"),
        (
            "docs/guides/setup.md",
            "!!! note \"Before you start\"\n    Check the cable.\n\nThen go.\n",
        ),
        ("docs/guides/vpn.md", "# VPN\n\nSee [setup](setup.md)."),
        ("docs/about.md", "About"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Mkdocs);
    assert_eq!(
        outline(&built),
        vec!["Home", "Guides", "  Setup", "  VPN", "About"]
    );
    assert_eq!(built.plan.collections[0].name, "IT Handbook");
    let setup = doc(&built, "Setup");
    assert_eq!(
        lines(setup),
        vec!["Before you start", "Check the cable.", "Then go."]
    );
    assert_eq!(
        destinations(doc(&built, "VPN")),
        vec![page_href(&built, "Setup")]
    );
}

#[test]
fn docusaurus_reads_categories_positions_and_mdx() {
    let files = text(&[
        ("docusaurus.config.js", "module.exports = {};"),
        (
            "docs/intro.md",
            "---\nsidebar_position: 1\n---\n# Introduction\n\n:::tip Remember\nSave often.\n:::\n\nAfter.\n",
        ),
        (
            "docs/02-guides/_category_.json",
            "{\"label\": \"User guides\", \"position\": 2}",
        ),
        (
            "docs/02-guides/01-install.mdx",
            "---\nsidebar_label: Install it\n---\nimport Tabs from '@theme/Tabs';\n\n<Tabs>\nSteps\n</Tabs>\n",
        ),
        ("docs/02-guides/02-configure.md", "Configure"),
        ("docs/faq.md", "---\nsidebar_position: 3\n---\nFAQ"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Docusaurus);
    assert_eq!(
        outline(&built),
        vec![
            "Introduction",
            "User guides",
            "  Install it",
            "  Configure",
            "Faq",
        ]
    );
    assert_eq!(
        lines(doc(&built, "Introduction")),
        vec!["Remember", "Save often.", "After."]
    );
    assert_eq!(lines(doc(&built, "Install it")), vec!["Steps"]);
    let install = &built.plan.pages[index_of(&built, "Install it")];
    assert_eq!(install.notes.count(NoteCode::SourceSyntaxRemoved), 3);
}

#[test]
fn an_outline_export_is_a_collection_per_folder() {
    let files = text(&[
        (
            "Engineering/Onboarding.md",
            "# Onboarding\n\n![Screen](uploads/abc/screen.png)",
        ),
        ("Engineering/Onboarding/Laptop.md", "Laptop"),
        ("Engineering/uploads/abc/screen.png", "png"),
        ("Support/Macros.md", "Macros"),
    ]);
    let built = build(&files, &options(), &nothing());
    assert_eq!(built.plan.profile, ProfileKind::Outline);
    assert_eq!(outline(&built), vec!["Onboarding", "  Laptop", "Macros"]);
    let names: Vec<&str> = built
        .plan
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["Engineering", "Support"]);
    let collections: Vec<Option<usize>> = built.plan.pages.iter().map(|p| p.collection).collect();
    assert_eq!(collections, vec![Some(0), None, Some(1)]);
    assert_eq!(
        destinations(doc(&built, "Onboarding")),
        vec![asset("Engineering/uploads/abc/screen.png")]
    );
}

#[test]
fn a_forced_profile_and_collection_mode_apply() {
    let files = text(&[("a/one.md", "One"), ("b/two.md", "Two"), ("top.md", "Top")]);
    let built = build(
        &files,
        &ImportOptions {
            profile: Some(ProfileKind::Obsidian),
            collections: Some(CollectionMode::PerTopLevelFolder),
            ..options()
        },
        &nothing(),
    );
    assert_eq!(built.plan.profile, ProfileKind::Obsidian);
    assert_eq!(built.plan.detected, Some(ProfileKind::Plain));
    let names: Vec<&str> = built
        .plan
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["A", "B", "IT-Handbook"]);
    assert_eq!(outline(&built), vec!["One", "Two", "Top"]);
}

// Refusals.

#[test]
fn bookstack_and_confluence_exports_are_refused_for_now() {
    let bookstack = text(&[
        (
            "data.json",
            "{\"instance\": {\"id\": \"x\"}, \"exported_at\": \"2026\", \"book\": {\"name\": \"B\"}}",
        ),
        ("files/a.png", "png"),
    ]);
    let err = tree::build(&mut archive(&bookstack), &options(), &nothing()).unwrap_err();
    assert_eq!(err.code, "docs_import_source_later");
    assert_eq!(
        err.message,
        "This is a BookStack export. BookStack and Confluence exports are coming in a later update."
    );

    let confluence = text(&[
        ("SPACE/index.html", "<html>Confluence</html>"),
        ("SPACE/Page_1.html", "<html></html>"),
        ("SPACE/attachments/1/a.png", "png"),
    ]);
    let err = tree::build(&mut archive(&confluence), &options(), &nothing()).unwrap_err();
    assert!(err.message.starts_with("This is a Confluence export."));
}

#[test]
fn an_archive_with_no_markdown_or_too_many_pages_is_refused() {
    let err = tree::build(
        &mut archive(&text(&[("a.png", "png")])),
        &options(),
        &nothing(),
    )
    .unwrap_err();
    assert_eq!(err.code, "docs_import_no_markdown");

    let names: Vec<String> = (0..=tree::MAX_PAGES).map(|i| format!("p{i}.md")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    let err = tree::build(&mut archive(&files), &options(), &nothing()).unwrap_err();
    assert_eq!(err.code, "docs_import_too_many_pages");
    assert_eq!(
        err.message,
        "The import has 2001 pages. The limit is 2000. Export one part at a time."
    );
}

// Front matter limits.

#[test]
fn front_matter_past_its_limits_is_ignored_with_a_note() {
    let big = format!(
        "---\ntitle: Big\nbody: \"{}\"\n---\n# From the heading\n",
        "a".repeat(17 * 1024)
    );
    let long = format!("---\ntitle: {}\n---\n", "T".repeat(300));
    let files: Vec<(&str, &[u8])> = vec![
        ("big.md", big.as_bytes()),
        (
            "alias.md",
            b"---\ntitle: &t Anchored\nother: *t\n---\n# Heading title\n",
        ),
        ("long.md", long.as_bytes()),
    ];
    let built = build(&files, &options(), &nothing());
    let big = &built.plan.pages[index_of(&built, "From the heading")];
    assert_eq!(big.notes.count(NoteCode::FrontMatterIgnored), 1);
    let alias = &built.plan.pages[index_of(&built, "Heading title")];
    assert_eq!(alias.notes.count(NoteCode::FrontMatterIgnored), 1);
    let long = built
        .plan
        .pages
        .iter()
        .find(|p| p.source.as_deref() == Some("long.md"))
        .unwrap();
    assert_eq!(long.title.chars().count(), tree::MAX_TITLE_CHARS);
    assert_eq!(long.notes.count(NoteCode::TitleShortened), 1);
}
