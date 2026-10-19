use std::collections::HashMap;

use serde_json::{json, Value};

use super::*;
use crate::services::docs_markdown::encode::{decode, encode_with_client_id};
use crate::services::docs_markdown::ir::{
    Block, Document, Image, Inline, ListItem, Marks, Table, TableCell,
};
use yrs::WriteTxn;

/// Targets from three maps; anything missing isn't on the target.
#[derive(Default)]
struct Map {
    tickets: HashMap<String, String>,
    users: HashMap<String, String>,
    pages: HashMap<String, String>,
}

impl Targets for Map {
    fn ticket(&self, id: &str) -> Option<String> {
        self.tickets.get(id).cloned()
    }
    fn user(&self, uuid: &str) -> Option<String> {
        self.users.get(uuid).cloned()
    }
    fn page(&self, uuid: &str) -> Option<String> {
        self.pages.get(uuid).cloned()
    }
}

const ANA: &str = "0190a000-0000-7000-8000-00000000000a";
const ANA_HERE: &str = "0190b000-0000-7000-8000-00000000000a";
const GONE: &str = "0190a000-0000-7000-8000-0000000000ff";
const PAGE: &str = "0190a000-0000-7000-8000-0000000000d1";
const PAGE_HERE: &str = "0190b000-0000-7000-8000-0000000000d1";
const PAGE_GONE: &str = "0190a000-0000-7000-8000-0000000000d2";

fn targets() -> Map {
    Map {
        tickets: HashMap::from([("41".to_string(), "9041".to_string())]),
        users: HashMap::from([(ANA.to_string(), ANA_HERE.to_string())]),
        pages: HashMap::from([(PAGE.to_string(), PAGE_HERE.to_string())]),
    }
}

fn mention(uuid: &str, name: &str, avatar: Option<&str>) -> Inline {
    Inline::Mention {
        uuid: uuid.into(),
        name: name.into(),
        avatar_url: avatar.map(str::to_string),
    }
}

fn image(src: &str, alt: &str) -> Inline {
    Inline::Image(Image {
        src: src.into(),
        alt: Some(alt.into()),
        ..Image::default()
    })
}

fn ticket(id: &str, href: &str) -> Inline {
    Inline::TicketLink {
        ticket_id: id.into(),
        href: href.into(),
    }
}

fn embed(uuid: &str, title: &str) -> Block {
    Block::EmbeddedDocument {
        document_uuid: uuid.into(),
        document_title: title.into(),
    }
}

/// Every node the rewrite reads, in the places the editor puts them: inline
/// in a paragraph, a list item and a table cell, and embeds at the top
/// level.
fn archive_document() -> Document {
    Document {
        blocks: vec![
            Block::Paragraph(vec![
                Inline::text("See "),
                ticket("41", "https://old.example/acme/tickets/id/41"),
                Inline::text(" and "),
                ticket("77", "https://old.example/acme/tickets/12"),
                Inline::text(", ask "),
                mention(ANA, "Ana", Some("/uploads/users/avatars/ana.png")),
                Inline::marked(" or ask", Marks::strong()),
                mention(GONE, "Gone", None),
                Inline::text(" today."),
            ]),
            Block::Paragraph(vec![
                image("/uploads/tickets/41/notes/a.png", "diagram"),
                Inline::text(" then "),
                image("/api/files/tickets/99/notes/b.png", "lost"),
                image("https://cdn.example/x.png", "outside"),
            ]),
            embed(PAGE, "Runbook"),
            embed(PAGE_GONE, "Old page"),
            Block::BulletList(vec![ListItem {
                blocks: vec![Block::Paragraph(vec![mention(GONE, "Gone", None)])],
            }]),
            Block::Table(Table {
                rows: vec![vec![TableCell::new(
                    false,
                    vec![mention(ANA, "Ana", None), Inline::text(" owns it")],
                )]],
            }),
        ],
    }
}

/// What the target should hold.
fn expected_document() -> Document {
    Document {
        blocks: vec![
            Block::Paragraph(vec![
                Inline::text("See "),
                ticket("9041", "/tickets/id/9041"),
                Inline::text(" and https://old.example/acme/tickets/12, ask "),
                mention(ANA_HERE, "Ana", None),
                Inline::marked(" or ask", Marks::strong()),
                Inline::text("@Gone today."),
            ]),
            Block::Paragraph(vec![
                image("/uploads/tickets/9041/notes/a.png", "diagram"),
                Inline::text(" then lost"),
                image("https://cdn.example/x.png", "outside"),
            ]),
            embed(PAGE_HERE, "Runbook"),
            Block::Paragraph(vec![Inline::text("Old page")]),
            Block::BulletList(vec![ListItem {
                blocks: vec![Block::Paragraph(vec![Inline::text("@Gone")])],
            }]),
            Block::Table(Table {
                rows: vec![vec![TableCell::new(
                    false,
                    vec![mention(ANA_HERE, "Ana", None), Inline::text(" owns it")],
                )]],
            }),
        ],
    }
}

/// ProseMirror JSON with adjacent text nodes of the same marks joined, as
/// ProseMirror joins them when it builds the document.
fn joined(value: Value) -> Value {
    match value {
        Value::Object(mut obj) => {
            if let Some(Value::Array(content)) = obj.remove("content") {
                let mut out: Vec<Value> = Vec::new();
                for child in content.into_iter().map(joined) {
                    let same = out.last().is_some_and(|last| {
                        last["type"] == "text"
                            && child["type"] == "text"
                            && last.get("marks") == child.get("marks")
                    });
                    if same {
                        let text = child["text"].as_str().unwrap_or_default().to_string();
                        if let Some(last) = out.last_mut() {
                            let joined = format!("{}{text}", last["text"].as_str().unwrap_or(""));
                            last["text"] = Value::String(joined);
                        }
                    } else {
                        out.push(child);
                    }
                }
                obj.insert("content".into(), Value::Array(out));
            }
            Value::Object(obj)
        }
        other => other,
    }
}

fn rewritten(update: &[u8], targets: &Map) -> (Vec<u8>, Vec<u8>, Counts) {
    match rewrite_with_client_id(update, targets, Some(2)) {
        Rewrite::Rewritten {
            update,
            state_vector,
            counts,
        } => (update, state_vector, counts),
        other => panic!("expected a rewrite, got {other:?}"),
    }
}

#[test]
fn every_reference_is_rewritten_and_the_unplaced_become_text() {
    let source = encode_with_client_id(&archive_document(), 1);
    let (update, state_vector, counts) = rewritten(&source, &targets());

    let got = joined(decode::try_to_json(&update).expect("the rewrite decodes"));
    let want = joined(decode::to_json(&encode_with_client_id(
        &expected_document(),
        1,
    )));
    assert_eq!(got, want);

    assert_eq!(
        counts,
        Counts {
            ticket_links: 1,
            mentions: 2,
            images: 1,
            embeds: 1,
            ticket_links_as_text: 1,
            mentions_as_text: 2,
            images_as_text: 1,
            embeds_as_text: 1,
        }
    );
    assert_eq!(counts.unresolved(), 5);

    // Each run of text is still one XmlText: "@Gone" joined the unformatted
    // text after it, not the bold run before it.
    let first = &got["content"][0]["content"];
    assert_eq!(first[5], json!({"type": "text", "text": "@Gone today."}));
    let doc = super::super::load(&update).expect("loads");
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).expect("fragment");
    let Some(XmlOut::Element(paragraph)) = fragment.get(&txn, 0) else {
        panic!("first block is a paragraph");
    };
    let texts = paragraph
        .children(&txn)
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| matches!(w, [XmlOut::Text(_), XmlOut::Text(_)]))
        .count();
    assert_eq!(texts, 0, "no two text runs side by side");

    // The stored state vector is the new document's.
    let fresh = super::super::encode(&doc).1;
    assert_eq!(state_vector, fresh);
}

#[test]
fn a_document_naming_nothing_that_moves_is_left_alone() {
    let source = encode_with_client_id(
        &Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::text("Plain "),
                image("https://cdn.example/x.png", "outside"),
            ])],
        },
        1,
    );
    assert_eq!(rewrite(&source, &targets()), Rewrite::Unchanged);

    // Identity targets: every reference resolves to itself.
    let same = Map {
        tickets: HashMap::from([("41".to_string(), "41".to_string())]),
        users: HashMap::from([(ANA.to_string(), ANA.to_string())]),
        pages: HashMap::from([(PAGE.to_string(), PAGE.to_string())]),
    };
    let source = encode_with_client_id(
        &Document {
            blocks: vec![
                Block::Paragraph(vec![
                    ticket("41", "/tickets/id/41"),
                    mention(ANA, "Ana", None),
                    image("/uploads/tickets/41/notes/a.png", "a"),
                ]),
                embed(PAGE, "Runbook"),
            ],
        },
        1,
    );
    assert_eq!(rewrite(&source, &same), Rewrite::Unchanged);
}

#[test]
fn bytes_that_are_not_a_document_are_reported() {
    assert_eq!(
        rewrite(b"not yjs at all", &targets()),
        Rewrite::NotADocument
    );
}

#[test]
fn a_numeric_ticket_id_stays_a_number() {
    let doc = Doc::with_client_id(1);
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
        let p = fragment.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        let link = p.push_back(&mut txn, XmlElementPrelim::empty("ticket_link"));
        link.insert_attribute(&mut txn, "ticketId", Any::Number(41.0));
        link.insert_attribute(&mut txn, "href", Any::from("/tickets/id/41"));
    }
    let (update, _) = super::super::encode(&doc);
    let (update, _, _) = rewritten(&update, &targets());
    let json = decode::to_json(&update);
    assert_eq!(
        json["content"][0]["content"][0]["attrs"]["ticketId"],
        json!(9041)
    );
}

#[test]
fn notes_paths() {
    assert_eq!(
        notes_image_ticket("/uploads/tickets/41/notes/a.png"),
        Some("41")
    );
    assert_eq!(
        notes_image_ticket("https://x.example/api/files/tickets/7/notes/b c.png"),
        Some("7")
    );
    assert_eq!(notes_image_ticket("/uploads/tickets/41/a.png"), None);
    assert_eq!(
        with_notes_image_ticket("/api/files/tickets/7/notes/b.png", "70"),
        "/api/files/tickets/70/notes/b.png"
    );
}

#[test]
fn a_mention_between_non_ascii_text_becomes_text_in_place() {
    let source = encode_with_client_id(
        &Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::marked("★ or ask é", Marks::strong()),
                mention(GONE, "Zoë", None),
                Inline::text(" tödäy 🙂."),
                ticket("77", "/tickets/12"),
                Inline::text(" 終わり"),
            ])],
        },
        1,
    );
    let (update, _, counts) = rewritten(&source, &targets());
    assert_eq!(counts.unresolved(), 2);
    let got = joined(decode::try_to_json(&update).expect("the rewrite decodes"));
    let want = joined(decode::to_json(&encode_with_client_id(
        &Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::marked("★ or ask é", Marks::strong()),
                Inline::text("@Zoë tödäy 🙂./tickets/12 終わり"),
            ])],
        },
        1,
    )));
    assert_eq!(got, want);
}

// The rewritten document is also a golden fixture beside the Markdown
// converter's, so `docsMarkdownFixtures.spec.ts` builds it with the real
// editor schema through y-prosemirror. Regenerate with
// `UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test services::yjs`.

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/docs_markdown/workspace-import-rewrite.yjs.json"
);

#[test]
fn the_rewritten_fixture_is_current() {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::STANDARD;
    let source = encode_with_client_id(&archive_document(), 1);
    let (fresh, _, _) = rewritten(&source, &targets());
    let stored = std::fs::read_to_string(FIXTURE)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["update"].as_str().map(str::to_string))
        .and_then(|b| engine.decode(b).ok());
    let current = stored
        .as_deref()
        .is_some_and(|s| decode::to_json(s) == decode::to_json(&fresh));
    if current {
        return;
    }
    if std::env::var_os("UPDATE_DOCS_MARKDOWN_FIXTURES").is_some() {
        let body = json!({
            "source": "workspace-import-rewrite",
            "notes": {},
            "update": engine.encode(&fresh),
        });
        let text = serde_json::to_string_pretty(&body).expect("serialises") + "\n";
        std::fs::write(FIXTURE, text).expect("fixture writes");
    } else {
        panic!(
            "stale fixture {FIXTURE}; run UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test services::yjs"
        );
    }
}
