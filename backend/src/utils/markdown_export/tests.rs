use std::collections::HashMap;

use serde_json::{json, Value};
use yrs::types::xml::{XmlDeltaPrelim, XmlIn};
use yrs::types::Delta;
use yrs::{
    Any, Doc, ReadTxn, StateVector, Transact, TransactionMut, WriteTxn, Xml, XmlElementPrelim,
    XmlFragment,
};

use super::*;
use crate::services::docs_markdown::encode::decode::to_json;
use crate::services::docs_markdown::ir::{
    Alignment, Block, Document, Image, Inline, Link, ListItem, Marks, Table, TableCell, Text,
};
use crate::services::docs_markdown::{self, normalise, DefaultResolver, Notes};

// Building documents from ProseMirror JSON, the way y-prosemirror writes
// them.

fn any(value: &Value) -> Any {
    match value {
        Value::Null => Any::Null,
        Value::Bool(b) => Any::Bool(*b),
        Value::Number(n) => Any::Number(n.as_f64().unwrap_or_default()),
        Value::String(s) => Any::from(s.as_str()),
        Value::Array(items) => Any::from(items.iter().map(any).collect::<Vec<_>>()),
        Value::Object(map) => Any::from(
            map.iter()
                .map(|(k, v)| (k.clone(), any(v)))
                .collect::<HashMap<String, Any>>(),
        ),
    }
}

fn write_children<P: XmlFragment>(txn: &mut TransactionMut, parent: &P, content: &[Value]) {
    let mut run: Vec<Delta<yrs::In>> = Vec::new();
    let flush = |txn: &mut TransactionMut, run: &mut Vec<Delta<yrs::In>>| {
        if !run.is_empty() {
            parent.push_back(
                txn,
                XmlIn::from(XmlDeltaPrelim {
                    attributes: HashMap::new(),
                    delta: std::mem::take(run),
                }),
            );
        }
    };
    for node in content {
        if node["type"] == "text" {
            let text = node["text"].as_str().unwrap_or_default();
            let marks = node["marks"].as_array().cloned().unwrap_or_default();
            if marks.is_empty() {
                run.push(Delta::insert(text));
            } else {
                let attrs = marks
                    .iter()
                    .map(|m| {
                        let name = m["type"].as_str().unwrap_or_default();
                        let value = m
                            .get("attrs")
                            .map(any)
                            .unwrap_or_else(|| Any::from(HashMap::<String, Any>::new()));
                        (std::sync::Arc::from(name), value)
                    })
                    .collect();
                run.push(Delta::insert_with(text, attrs));
            }
            continue;
        }
        flush(txn, &mut run);
        let el = parent.push_back(
            txn,
            XmlElementPrelim::empty(node["type"].as_str().unwrap_or_default()),
        );
        if let Some(attrs) = node["attrs"].as_object() {
            for (key, value) in attrs {
                if !value.is_null() {
                    el.insert_attribute(txn, key.as_str(), any(value));
                }
            }
        }
        let inner = node["content"].as_array().cloned().unwrap_or_default();
        write_children(txn, &el, &inner);
    }
    flush(txn, &mut run);
}

/// A y-prosemirror Yjs update from a ProseMirror document.
fn yjs(doc: Value) -> Vec<u8> {
    let ydoc = Doc::new();
    {
        let mut txn = ydoc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        let content = doc["content"].as_array().cloned().unwrap_or_default();
        write_children(&mut txn, &fragment, &content);
    }
    let txn = ydoc.transact();
    txn.encode_state_as_update_v1(&StateVector::default())
}

fn md(doc: Value) -> String {
    yjs_to_markdown(&yjs(doc)).unwrap_or_default()
}

fn text(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}

fn marked(s: &str, marks: Value) -> Value {
    json!({ "type": "text", "text": s, "marks": marks })
}

fn para(content: Vec<Value>) -> Value {
    json!({ "type": "paragraph", "content": content })
}

fn doc(content: Vec<Value>) -> Value {
    json!({ "type": "doc", "content": content })
}

/// The document the importer reads back from `markdown`, as JSON.
fn reimported(markdown: &str) -> Value {
    to_json(&docs_markdown::encode(
        &docs_markdown::convert(markdown).document,
    ))
}

// Inline marks.

#[test]
fn bold_and_a_link_come_out_as_markdown_not_tags() {
    let out = md(doc(vec![para(vec![
        text("plain "),
        marked("bold", json!([{ "type": "strong" }])),
        text(" and "),
        marked(
            "a link",
            json!([{ "type": "link", "attrs": { "href": "https://x.example", "title": null } }]),
        ),
    ])]));
    assert_eq!(out, "plain **bold** and [a link](https://x.example)");
}

#[test]
fn overlapping_marks_nest_without_reopening() {
    let out = md(doc(vec![para(vec![
        marked("a", json!([{ "type": "strong" }])),
        marked("b", json!([{ "type": "strong" }, { "type": "em" }])),
        marked("c", json!([{ "type": "strong" }])),
    ])]));
    assert_eq!(out, "**a*b*c**");
}

#[test]
fn legacy_true_marks_and_hashed_mark_names_are_read() {
    let out = md(doc(vec![para(vec![
        marked("old", json!([{ "type": "strong", "attrs": true }])),
        text(" "),
        marked("new", json!([{ "type": "em--a1b2c3" }])),
    ])]));
    assert_eq!(out, "**old** *new*");
}

#[test]
fn a_mark_markdown_cannot_close_is_written_as_a_tag() {
    // `**a **` doesn't parse as bold: the closer follows a space.
    let source = doc(vec![para(vec![
        marked("bold ", json!([{ "type": "strong" }])),
        text("then plain"),
    ])]);
    let out = md(source.clone());
    assert_eq!(out, "<strong>bold </strong>then plain");
    assert_eq!(reimported(&out), to_json(&yjs(source)));
}

#[test]
fn code_spans_fence_around_backticks() {
    let out = md(doc(vec![para(vec![
        marked("a`b", json!([{ "type": "code" }])),
        text(" "),
        marked("`x", json!([{ "type": "code" }])),
    ])]));
    assert_eq!(out, "``a`b`` `` `x ``");
}

// Escaping.

#[test]
fn markdown_significant_text_is_escaped() {
    let cases = [
        "# not a heading",
        "- not a list",
        "1. not a list",
        "> not a quote",
        "*not em* and **not strong**",
        "[not](a link) ![nor](an image)",
        "`not code`",
        "<b>not html</b> &amp; not an entity",
        "a | b | c",
        "~~not struck~~ $not math$",
        "  leading and trailing spaces  ",
        "back\\slash \\*",
        "snake_case stays and _this_ is escaped",
        "===",
        ": not a definition",
        "Hello!",
        "[!NOTE]",
    ];
    for case in cases {
        let source = doc(vec![para(vec![text(case)])]);
        let out = md(source.clone());
        assert_eq!(
            reimported(&out),
            to_json(&yjs(source)),
            "{case:?} as {out:?}"
        );
    }
    assert_eq!(
        md(doc(vec![para(vec![text("snake_case stays")])])),
        "snake_case stays"
    );
}

#[test]
fn a_hard_break_continues_the_line_or_ends_it_with_br() {
    assert_eq!(
        md(doc(vec![para(vec![
            text("one"),
            json!({ "type": "hard_break" }),
            text("- two"),
            json!({ "type": "hard_break" }),
        ])])),
        "one\\\n\\- two<br>"
    );
}

// Nesting.

#[test]
fn lists_quotes_and_code_nest_by_indentation() {
    let source = doc(vec![
        json!({ "type": "ordered_list", "attrs": { "order": 9 }, "content": [
            { "type": "list_item", "content": [
                para(vec![text("nine")]),
                { "type": "bullet_list", "content": [
                    { "type": "list_item", "content": [
                        para(vec![text("inner")]),
                        { "type": "code_block", "attrs": { "language": "sh" }, "content": [text("echo 1\n\n  echo 2")] },
                    ]},
                ]},
            ]},
            { "type": "list_item", "content": [
                para(vec![text("ten")]),
                { "type": "blockquote", "content": [para(vec![text("quoted")]), para(vec![text("twice")])] },
            ]},
        ]}),
        json!({ "type": "blockquote", "content": [
            { "type": "bullet_list", "content": [
                { "type": "list_item", "content": [para(vec![text("in a quote")])] },
            ]},
            { "type": "code_block", "content": [text("a\n\nb")] },
        ]}),
    ]);
    let out = md(source.clone());
    assert_eq!(
        out,
        "9. nine\n   - inner\n\n     ```sh\n     echo 1\n\n       echo 2\n     ```\n\n\
         10. ten\n\n    > quoted\n    >\n    > twice\n\n\
         > - in a quote\n>\n> ```\n> a\n>\n> b\n> ```"
    );
    assert_eq!(reimported(&out), to_json(&yjs(source)));
}

#[test]
fn two_lists_in_a_row_stay_two_lists() {
    let item = |s: &str| json!({ "type": "list_item", "content": [para(vec![text(s)])] });
    let source = doc(vec![
        json!({ "type": "bullet_list", "content": [item("a")] }),
        json!({ "type": "bullet_list", "content": [item("b")] }),
        json!({ "type": "ordered_list", "attrs": { "order": 1 }, "content": [item("c")] }),
        json!({ "type": "ordered_list", "attrs": { "order": 1 }, "content": [item("d")] }),
    ]);
    let out = md(source.clone());
    assert_eq!(out, "- a\n\n* b\n\n1. c\n\n1) d");
    assert_eq!(reimported(&out), to_json(&yjs(source)));
}

#[test]
fn a_rule_opening_the_document_is_not_front_matter() {
    let source = doc(vec![
        json!({ "type": "horizontal_rule" }),
        para(vec![text("x")]),
        json!({ "type": "horizontal_rule" }),
    ]);
    let out = md(source.clone());
    assert_eq!(out, "***\n\nx\n\n***");
    assert_eq!(reimported(&out), to_json(&yjs(source)));
}

// The editor's own nodes.

#[test]
fn mentions_ticket_links_and_embeds_are_the_schemas_elements() {
    let out = md(doc(vec![
        para(vec![
            json!({ "type": "mention", "attrs": { "uuid": "u-1", "name": "Ana \"A\" <ops>" } }),
            text(" see "),
            json!({ "type": "ticket_link", "attrs": { "ticketId": "42", "href": "/tickets/7" } }),
        ]),
        json!({ "type": "embedded_document", "attrs": { "documentUuid": "d-1", "documentTitle": "VPN & Wi-Fi" } }),
    ]));
    assert_eq!(
        out,
        "<span data-mention=\"true\" data-uuid=\"u-1\" data-name=\"Ana &quot;A&quot; &lt;ops&gt;\">@Ana \"A\" \\<ops></span> see \
         <span data-ticket-link=\"true\" data-ticket-id=\"42\" data-href=\"/tickets/7\">Ticket</span>\n\n\
         <div data-embedded-document=\"true\" data-document-uuid=\"d-1\" data-document-title=\"VPN &amp; Wi-Fi\">VPN &amp; Wi-Fi</div>"
    );
}

/// A document whose one paragraph links ticket 42.
fn linking_ticket_42() -> Doc {
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        let mut link = XmlElementPrelim::new("ticket_link", Vec::<XmlIn>::new());
        link.attributes.insert("ticketId".into(), "42".to_string());
        link.attributes
            .insert("href".into(), "/tickets/7".to_string());
        fragment.push_back(
            &mut txn,
            XmlElementPrelim::new("paragraph", vec![link.into()]),
        );
    }
    doc
}

#[test]
fn a_ticket_link_quotes_the_tickets_number() {
    let doc = linking_ticket_42();
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror").expect("fragment");
    let numbers = TicketNumbers::from([(42, 7)]);
    let mut writer = Writer {
        txn: &txn,
        numbers: &numbers,
        links: &KeepLinks,
        embeds: Embeds::Element,
    };
    assert_eq!(
        writer.fragment(&fragment),
        "<span data-ticket-link=\"true\" data-ticket-id=\"42\" data-href=\"/tickets/7\">Ticket #7</span>"
    );
}

#[test]
fn a_ticket_link_without_a_number_is_named_by_id() {
    let doc = linking_ticket_42();
    let update = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    assert_eq!(
        yjs_to_markdown(&update).as_deref(),
        Some(
            "<span data-ticket-link=\"true\" data-ticket-id=\"42\" data-href=\"/tickets/7\">Ticket</span>"
        )
    );
}

#[test]
fn an_image_with_a_width_keeps_it_as_the_editors_element() {
    let out = md(doc(vec![para(vec![
        json!({ "type": "image", "attrs": { "src": "/a.png", "alt": "A", "width": 240, "align": "center" } }),
        text(" "),
        json!({ "type": "image", "attrs": { "src": "/b c.png", "title": "B \"t\"" } }),
    ])]));
    assert_eq!(
        out,
        "<img src=\"/a.png\" alt=\"A\" width=\"240\" data-align=\"center\"> ![](</b c.png> \"B \\\"t\\\"\")"
    );
}

#[test]
fn links_rewrite_hrefs_and_image_sources() {
    struct Rewrite;
    impl Links for Rewrite {
        fn href(&self, href: &str) -> String {
            href.replace("/documentation/vpn", "../it/vpn.md")
        }
        fn image_src(&self, src: &str) -> String {
            src.replace("/api/files/collab/doc/", "_assets/")
        }
    }
    let bytes = yjs(doc(vec![para(vec![
        marked(
            "VPN",
            json!([{ "type": "link", "attrs": { "href": "/documentation/vpn#setup" } }]),
        ),
        json!({ "type": "image", "attrs": { "src": "/api/files/collab/doc/u/1_a.png" } }),
    ])]));
    let doc = load(&bytes).expect("doc");
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).expect("fragment");
    let numbers = TicketNumbers::new();
    let mut writer = Writer {
        txn: &txn,
        numbers: &numbers,
        links: &Rewrite,
        embeds: Embeds::Element,
    };
    assert_eq!(
        writer.fragment(&fragment),
        "[VPN](../it/vpn.md#setup)![](_assets/u/1_a.png)"
    );
}

// Tables.

fn cell(kind: &str, content: Vec<Value>, attrs: Value) -> Value {
    json!({ "type": kind, "attrs": attrs, "content": content })
}

fn th(s: &str) -> Value {
    cell(
        "table_header",
        vec![para(vec![text(s)])],
        json!({ "colspan": 1, "rowspan": 1 }),
    )
}

fn td(s: &str) -> Value {
    cell(
        "table_cell",
        vec![para(vec![text(s)])],
        json!({ "colspan": 1, "rowspan": 1 }),
    )
}

fn row(cells: Vec<Value>) -> Value {
    json!({ "type": "table_row", "content": cells })
}

fn table(rows: Vec<Value>) -> Value {
    doc(vec![json!({ "type": "table", "content": rows })])
}

#[test]
fn an_exact_table_is_a_pipe_table() {
    let aligned = |kind: &str, content: Vec<Value>, align: &str| {
        cell(
            kind,
            vec![para(content)],
            json!({ "colspan": 1, "rowspan": 1, "align": align, "colwidth": [120] }),
        )
    };
    let out = md(table(vec![
        row(vec![
            th("System"),
            aligned("table_header", vec![text("Status")], "center"),
        ]),
        row(vec![
            cell(
                "table_cell",
                vec![para(vec![
                    text("a | b"),
                    json!({ "type": "hard_break" }),
                    marked("x|y", json!([{ "type": "code" }])),
                ])],
                json!({ "colspan": 1, "rowspan": 1, "colwidth": [120] }),
            ),
            aligned(
                "table_cell",
                vec![marked("Up", json!([{ "type": "strong" }]))],
                "center",
            ),
        ]),
        row(vec![
            cell("table_cell", vec![para(vec![])], json!({})),
            aligned("table_cell", vec![text(" - ")], "center"),
        ]),
    ]));
    assert_eq!(
        out,
        "| System | Status |\n| --- | :---: |\n| a \\| b<br>`x\\|y` | **Up** |\n|  | &#32;-&#32; |"
    );
    assert!(!out.contains("120"), "widths are never written: {out}");

    // GFM reads the escaped pipes back as text, inside code too.
    let mut cells = Vec::new();
    let mut current = String::new();
    for event in Parser::new_ext(&out, crate::services::docs_markdown::parse::options()) {
        match event {
            Event::Text(t) | Event::Code(t) => current.push_str(&t),
            Event::End(TagEnd::TableCell) => cells.push(std::mem::take(&mut current)),
            _ => {}
        }
    }
    assert_eq!(cells, ["System", "Status", "a | bx|y", "Up", "", " - "]);
}

#[test]
fn a_table_a_pipe_table_cannot_hold_is_an_html_block() {
    let html_cases = [
        (
            "no header row",
            table(vec![
                row(vec![td("a"), td("b")]),
                row(vec![td("c"), td("d")]),
            ]),
        ),
        (
            "a header cell below the first row",
            table(vec![
                row(vec![th("a"), th("b")]),
                row(vec![th("c"), td("d")]),
            ]),
        ),
        (
            "a span",
            table(vec![
                row(vec![cell(
                    "table_header",
                    vec![para(vec![text("wide")])],
                    json!({ "colspan": 2, "rowspan": 1 }),
                )]),
                row(vec![td("c"), td("d")]),
            ]),
        ),
        (
            "two paragraphs in a cell",
            table(vec![
                row(vec![th("a")]),
                row(vec![cell(
                    "table_cell",
                    vec![para(vec![text("one")]), para(vec![text("two")])],
                    json!({}),
                )]),
            ]),
        ),
        (
            "a list in a cell",
            table(vec![
                row(vec![th("a")]),
                row(vec![cell(
                    "table_cell",
                    vec![json!({ "type": "bullet_list", "content": [
                        { "type": "list_item", "content": [para(vec![text("x")])] }
                    ]})],
                    json!({}),
                )]),
            ]),
        ),
        (
            "two alignments in a column",
            table(vec![
                row(vec![cell(
                    "table_header",
                    vec![para(vec![text("a")])],
                    json!({ "align": "left" }),
                )]),
                row(vec![cell(
                    "table_cell",
                    vec![para(vec![text("b")])],
                    json!({ "align": "right" }),
                )]),
            ]),
        ),
        (
            "a short row",
            table(vec![row(vec![th("a"), th("b")]), row(vec![td("c")])]),
        ),
    ];
    for (case, source) in html_cases {
        let out = md(source);
        assert!(
            out.starts_with("<table>\n") && out.ends_with("\n</table>"),
            "{case}: {out}"
        );
        assert!(
            !out.contains("\n\n"),
            "{case}: a blank line ends the block: {out}"
        );
    }

    let out = md(table(vec![
        row(vec![cell(
            "table_header",
            vec![para(vec![text("wide")])],
            json!({ "colspan": 2, "rowspan": 1, "align": "center", "colwidth": [100, 80] }),
        )]),
        row(vec![
            cell(
                "table_cell",
                vec![
                    para(vec![
                        marked("bold & <b>", json!([{ "type": "strong" }])),
                        json!({ "type": "hard_break" }),
                        marked(
                            "link",
                            json!([{ "type": "link", "attrs": { "href": "https://x.example/?a=1&b=2" } }]),
                        ),
                    ]),
                    json!({ "type": "code_block", "attrs": { "language": "sh" }, "content": [text("a\n\nb")] }),
                ],
                json!({}),
            ),
            cell(
                "table_cell",
                vec![
                    json!({ "type": "ordered_list", "attrs": { "order": 3 }, "content": [
                        { "type": "list_item", "content": [para(vec![text("x")])] }
                    ]}),
                ],
                json!({}),
            ),
        ]),
    ]));
    assert_eq!(
        out,
        "<table>\n\
         <tr><th colspan=\"2\" align=\"center\"><p>wide</p></th></tr>\n\
         <tr><td><p><strong>bold &amp; &lt;b&gt;</strong><br><a href=\"https://x.example/?a=1&amp;b=2\">link</a></p>\
         <pre><code class=\"language-sh\">a&#10;&#10;b</code></pre></td>\
         <td><ol start=\"3\"><li><p>x</p></li></ol></td></tr>\n\
         </table>"
    );
}

/// A pipe table through the importer.
#[test]
fn a_pipe_table_round_trips() {
    let header = |s: &str, align: Option<&str>| {
        let mut attrs = json!({ "colspan": 1, "rowspan": 1 });
        if let Some(align) = align {
            attrs["align"] = json!(align);
        }
        cell("table_header", vec![para(vec![text(s)])], attrs)
    };
    let body = |content: Vec<Value>, align: Option<&str>| {
        let mut attrs = json!({ "colspan": 1, "rowspan": 1 });
        if let Some(align) = align {
            attrs["align"] = json!(align);
        }
        cell("table_cell", vec![para(content)], attrs)
    };
    let source = table(vec![
        row(vec![header("Name", None), header("State", Some("right"))]),
        row(vec![
            body(
                vec![
                    text("a | b"),
                    json!({ "type": "hard_break" }),
                    marked("x|y", json!([{ "type": "code" }])),
                ],
                None,
            ),
            body(
                vec![marked("Up", json!([{ "type": "strong" }]))],
                Some("right"),
            ),
        ]),
    ]);
    let out = md(source.clone());
    assert_eq!(reimported(&out), to_json(&yjs(source)), "{out}");
}

// The round trip: any document the importer could have written comes back
// unchanged from its export. Mentions, ticket links, embedded pages, image
// widths and the tables a pipe table can't hold (spans, header columns,
// several blocks in a cell) are written as HTML elements, which the importer
// reads as text until it reads those elements; they are left out of the
// generated documents until then. Column widths are never exported.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn percent(&mut self, p: usize) -> bool {
        self.below(100) < p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Text pieces chosen to collide with Markdown syntax.
const PIECES: &[&str] = &[
    "a",
    "word",
    "Ünï",
    "日本",
    " ",
    "  ",
    "\t",
    "*",
    "**",
    "_",
    "__",
    "snake_case",
    "`",
    "``",
    "[",
    "]",
    "(",
    ")",
    "<",
    ">",
    "&",
    "&amp;",
    "&#35;",
    "&copy",
    "#",
    "# ",
    "-",
    "- ",
    "+",
    "=",
    "===",
    ":",
    ": ",
    "1.",
    "2)",
    "!",
    "\\",
    "\\*",
    "|",
    "~",
    "~~",
    "$",
    "$x$",
    "https://e.example",
    "www.e.example",
    "a@b.example",
    "\"",
    "'",
    "{",
    "}",
    "^",
    "%",
    ".",
    ",",
    "[!NOTE]",
    "<b>",
    "</span>",
    "---",
    "***",
    "+++",
    "    ",
    "[^1]",
    "[[x]]",
    "==hi==",
    "<!-- c -->",
];

const HREFS: &[&str] = &[
    "https://example.com",
    "https://example.com/a_(b)",
    "https://example.com/?q=1&amp;r=2",
    "https://example.com/a b",
    "https://example.com/<x>",
    "https://example.com/\\path",
    "mailto:ops@example.com",
    "tel:+61200000000",
];

const IMAGE_SRCS: &[&str] = &[
    "https://img.example/a.png",
    "https://img.example/a b (1).png",
    "https://img.example/x?y=1&z=2",
];

const TITLES: &[&str] = &["Title", "with \"quotes\"", "back\\slash", "&amp; entity"];

fn gen_text(rng: &mut Rng) -> String {
    (0..=rng.below(4)).map(|_| *rng.pick(PIECES)).collect()
}

fn gen_marks(rng: &mut Rng) -> Marks {
    let code = rng.percent(15);
    Marks {
        link: rng.percent(15).then(|| Link {
            href: rng.pick(HREFS).to_string(),
            title: rng.percent(30).then(|| rng.pick(TITLES).to_string()),
        }),
        // The importer keeps no bold or italic on code.
        em: !code && rng.percent(25),
        strong: !code && rng.percent(25),
        code,
    }
}

fn gen_inlines(rng: &mut Rng) -> Vec<Inline> {
    (0..=rng.below(6))
        .map(|_| match rng.below(10) {
            0 => Inline::Image(Image {
                src: rng.pick(IMAGE_SRCS).to_string(),
                alt: rng.percent(70).then(|| gen_text(rng)),
                title: rng.percent(30).then(|| rng.pick(TITLES).to_string()),
                ..Image::default()
            }),
            1 => Inline::HardBreak,
            _ => Inline::Text(Text {
                text: gen_text(rng),
                marks: gen_marks(rng),
            }),
        })
        .collect()
}

fn gen_code(rng: &mut Rng) -> Block {
    const LINES: &[&str] = &[
        "fn main() {}",
        "",
        "    indented",
        "```",
        "````",
        "~~~",
        "  - not a list",
        "> not a quote",
        "<div>",
        "\t tab",
    ];
    let mut text: Vec<&str> = (0..=rng.below(4)).map(|_| *rng.pick(LINES)).collect();
    if rng.percent(10) {
        text.push("");
    }
    Block::CodeBlock {
        language: rng
            .percent(50)
            .then(|| rng.pick(&["rust", "sh", "c++", "objective-c"]).to_string()),
        text: text.join("\n"),
    }
}

/// A table a pipe table holds: a header row, then body rows as wide, one
/// paragraph per cell, one alignment per column.
fn gen_table(rng: &mut Rng) -> Block {
    let columns = 1 + rng.below(4);
    let alignments: Vec<Option<Alignment>> = (0..columns)
        .map(|_| {
            *rng.pick(&[
                None,
                None,
                Some(Alignment::Left),
                Some(Alignment::Center),
                Some(Alignment::Right),
            ])
        })
        .collect();
    let rows = (0..=rng.below(4))
        .map(|r| {
            alignments
                .iter()
                .map(|align| {
                    let inlines = if rng.percent(15) {
                        Vec::new()
                    } else {
                        gen_inlines(rng)
                    };
                    TableCell {
                        align: *align,
                        ..TableCell::new(r == 0, inlines)
                    }
                })
                .collect()
        })
        .collect();
    Block::Table(Table { rows })
}

fn gen_items(rng: &mut Rng, depth: usize) -> Vec<ListItem> {
    (0..=rng.below(3))
        .map(|_| {
            let mut blocks = vec![Block::Paragraph(if rng.percent(10) {
                Vec::new()
            } else {
                gen_inlines(rng)
            })];
            blocks.extend(gen_blocks(rng, depth + 1, 2));
            ListItem { blocks }
        })
        .collect()
}

fn gen_blocks(rng: &mut Rng, depth: usize, most: usize) -> Vec<Block> {
    (0..rng.below(most + 1))
        .map(|_| {
            let roll = if depth >= 3 {
                rng.below(6)
            } else {
                rng.below(10)
            };
            match roll {
                0..=2 => Block::Paragraph(gen_inlines(rng)),
                3 => Block::Heading {
                    level: 1 + rng.below(6) as u8,
                    content: gen_inlines(rng),
                },
                4 => gen_code(rng),
                5 => Block::HorizontalRule,
                6 => Block::Blockquote({
                    let mut inner = gen_blocks(rng, depth + 1, 3);
                    inner.insert(0, Block::Paragraph(gen_inlines(rng)));
                    inner
                }),
                7 => Block::BulletList(gen_items(rng, depth)),
                _ => Block::OrderedList {
                    order: *rng.pick(&[0, 1, 1, 2, 9, 10, 99, 123_456]),
                    items: gen_items(rng, depth),
                },
            }
        })
        .collect()
}

/// A schema-valid document in the importer's normal form: what it would
/// write, so any difference after the round trip is the export's.
fn gen_document(rng: &mut Rng) -> Document {
    let mut blocks = gen_blocks(rng, 0, 6);
    // Tables sit at the top level only.
    if rng.percent(40) {
        let at = rng.below(blocks.len() + 1);
        blocks.insert(at, gen_table(rng));
    }
    blocks.push(Block::Paragraph(gen_inlines(rng)));
    normalise::normalise(Document { blocks }, &DefaultResolver, &mut Notes::default())
}

/// Leaves out what doesn't come back yet:
///
/// - A paragraph of nothing but line breaks: `<br>` alone on a line is an
///   HTML block, which the importer reads as text.
/// - A quote whose first text starts `[!`: the importer reads an Obsidian
///   callout from the unescaped text, so `> \[!NOTE\]` still becomes one.
///
/// Empty paragraphs and bold or italic on code are already gone after
/// normalising.
fn representable(document: &Document) -> bool {
    fn inlines_ok(inlines: &[Inline]) -> bool {
        inlines.is_empty() || !inlines.iter().all(|i| matches!(i, Inline::HardBreak))
    }
    fn reads_as_callout(blocks: &[Block]) -> bool {
        matches!(
            blocks.first(),
            Some(Block::Paragraph(inlines))
                if matches!(inlines.first(), Some(Inline::Text(t)) if t.text.starts_with("[!"))
        )
    }
    fn blocks_ok(blocks: &[Block]) -> bool {
        blocks.iter().all(|block| match block {
            Block::Paragraph(inlines) => inlines_ok(inlines),
            Block::Heading { .. } => true,
            Block::Blockquote(inner) => !reads_as_callout(inner) && blocks_ok(inner),
            Block::BulletList(items) | Block::OrderedList { items, .. } => {
                items.iter().all(|item| blocks_ok(&item.blocks))
            }
            _ => true,
        })
    }
    blocks_ok(&document.blocks)
}

#[test]
fn any_document_round_trips_through_its_export() {
    let mut checked = 0;
    let mut with_tables = 0;
    for seed in 1..=3000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let source = gen_document(&mut rng);
        if !representable(&source) {
            continue;
        }
        let bytes = docs_markdown::encode(&source);
        let markdown = yjs_to_markdown(&bytes).unwrap_or_default();
        let expected = to_json(&bytes);
        let actual = reimported(&markdown);
        assert_eq!(
            actual, expected,
            "seed {seed}\n--- markdown ---\n{markdown}\n--- end ---"
        );
        checked += 1;
        if source.blocks.iter().any(|b| matches!(b, Block::Table(_))) {
            with_tables += 1;
        }
    }
    assert!(checked > 2000, "only {checked} documents checked");
    assert!(
        with_tables > 600,
        "only {with_tables} documents with a table"
    );
}

#[test]
fn the_welcome_page_round_trips_through_its_export() {
    let welcome = include_str!("../../../seeds/getting-started.md");
    let (bytes, _) = docs_markdown::markdown_to_yjs(welcome);
    let markdown = yjs_to_markdown(&bytes).expect("markdown");
    assert_eq!(reimported(&markdown), to_json(&bytes), "{markdown}");
}
