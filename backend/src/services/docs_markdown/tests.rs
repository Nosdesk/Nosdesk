//! One test per row of the mapping and per content rule, the encoder's
//! y-prosemirror conventions, and the golden fixtures the frontend checks
//! against the real schema.

use serde_json::{json, Value};

use super::encode::{decode, encode, encode_with_client_id};
use super::ir::{
    Alignment, Block, Document, Image, Inline, Link, ListItem, Marks, Table, TableCell, Text,
};
use super::normalise::{normalise, MAX_DEPTH};
use super::{convert, DefaultResolver, FrontMatterKind, NoteCode, Notes};

fn blocks(markdown: &str) -> Vec<Block> {
    convert(markdown).document.blocks
}

fn notes(markdown: &str) -> Notes {
    convert(markdown).notes
}

fn json(markdown: &str) -> Value {
    decode::to_json(&encode(&convert(markdown).document))
}

fn para(inlines: Vec<Inline>) -> Block {
    Block::Paragraph(inlines)
}

fn text(s: &str) -> Inline {
    Inline::text(s)
}

fn strong(s: &str) -> Inline {
    Inline::marked(s, Marks::strong())
}

fn em(s: &str) -> Inline {
    Inline::marked(
        s,
        Marks {
            em: true,
            ..Marks::default()
        },
    )
}

fn code(s: &str) -> Inline {
    Inline::marked(s, Marks::code())
}

fn linked(s: &str, href: &str, title: Option<&str>) -> Inline {
    Inline::marked(
        s,
        Marks {
            link: Some(Link {
                href: href.into(),
                title: title.map(Into::into),
            }),
            ..Marks::default()
        },
    )
}

fn item(blocks: Vec<Block>) -> ListItem {
    ListItem { blocks }
}

fn code_block(language: Option<&str>, text: &str) -> Block {
    Block::CodeBlock {
        language: language.map(Into::into),
        text: text.into(),
    }
}

fn normalised(blocks: Vec<Block>) -> (Vec<Block>, Notes) {
    let mut notes = Notes::default();
    let doc = normalise(Document { blocks }, &DefaultResolver, &mut notes);
    (doc.blocks, notes)
}

// Mapping rows.

#[test]
fn paragraph_and_soft_break_become_a_space() {
    assert_eq!(
        blocks("one\ntwo\n\nthree"),
        vec![para(vec![text("one two")]), para(vec![text("three")])]
    );
}

#[test]
fn atx_and_setext_headings_keep_their_level() {
    let md =
        "# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six\n\nSetext\n===\n\nSub\n---\n";
    let levels: Vec<u8> = blocks(md)
        .iter()
        .filter_map(|b| match b {
            Block::Heading { level, .. } => Some(*level),
            _ => None,
        })
        .collect();
    assert_eq!(levels, vec![1, 2, 3, 4, 5, 6, 1, 2]);
}

#[test]
fn strong_em_and_code_marks() {
    assert_eq!(
        blocks("**a** __b__ *c* _d_ `e`"),
        vec![para(vec![
            strong("a"),
            text(" "),
            strong("b"),
            text(" "),
            em("c"),
            text(" "),
            em("d"),
            text(" "),
            code("e"),
        ])]
    );
}

#[test]
fn code_text_carries_no_other_mark() {
    assert_eq!(
        blocks("***`x`*** [`y`](https://e.com)"),
        vec![para(vec![
            code("x"),
            text(" "),
            Inline::marked(
                "y",
                Marks {
                    code: true,
                    ..match linked("y", "https://e.com", None) {
                        Inline::Text(t) => t.marks,
                        _ => unreachable!(),
                    }
                }
            ),
        ])]
    );
}

#[test]
fn hard_breaks_from_spaces_backslash_and_br() {
    assert_eq!(
        blocks("a  \nb\\\nc<br>d"),
        vec![para(vec![
            text("a"),
            Inline::HardBreak,
            text("b"),
            Inline::HardBreak,
            text("c"),
            Inline::HardBreak,
            text("d"),
        ])]
    );
}

#[test]
fn links_keep_web_mail_and_phone_destinations() {
    assert_eq!(
        blocks("[a](https://e.com \"T\") <https://f.com> <x@y.com> [p](tel:+61200) [m](MAILTO:z@y.com)"),
        vec![para(vec![
            linked("a", "https://e.com", Some("T")),
            text(" "),
            linked("https://f.com", "https://f.com", None),
            text(" "),
            linked("x@y.com", "mailto:x@y.com", None),
            text(" "),
            linked("p", "tel:+61200", None),
            text(" "),
            linked("m", "MAILTO:z@y.com", None),
        ])]
    );
}

#[test]
fn other_link_destinations_keep_their_text() {
    let md = "[a](javascript:alert(1)) [b](../other.md) [c](#top) [d]() [e](https:///x) [f](file:///etc)";
    assert_eq!(blocks(md), vec![para(vec![text("a b c d e f")])]);
    assert_eq!(notes(md).count(NoteCode::LinksDropped), 6);
}

#[test]
fn a_dropped_link_split_across_marks_counts_once() {
    assert_eq!(
        notes("[plain **bold** plain](relative.md)").count(NoteCode::LinksDropped),
        1
    );
}

#[test]
fn images_keep_alt_and_title() {
    assert_eq!(
        blocks("![Rack](https://e.com/r.png \"Front\")"),
        vec![para(vec![Inline::Image(Image {
            src: "https://e.com/r.png".into(),
            alt: Some("Rack".into()),
            title: Some("Front".into()),
            ..Image::default()
        })])]
    );
}

#[test]
fn an_image_the_page_cannot_show_keeps_its_alt_text() {
    let md = "![Diagram **one**](img/d.png) ![](data:image/png;base64,AA==)";
    assert_eq!(blocks(md), vec![para(vec![text("Diagram one ")])]);
    assert_eq!(notes(md).count(NoteCode::ImagesDropped), 2);
}

#[test]
fn a_linked_image_keeps_the_image() {
    assert_eq!(
        blocks("[![logo](https://e.com/l.png)](https://e.com)"),
        vec![para(vec![Inline::Image(Image {
            src: "https://e.com/l.png".into(),
            alt: Some("logo".into()),
            ..Image::default()
        })])]
    );
}

#[test]
fn fenced_code_language_is_the_first_word_lowercased() {
    assert_eq!(
        blocks("```Rust title=\"x\"\nfn main() {}\n```\n\n```\nplain\n```\n\n```{weird}\nz\n```"),
        vec![
            code_block(Some("rust"), "fn main() {}"),
            code_block(None, "plain"),
            code_block(None, "z"),
        ]
    );
}

#[test]
fn indented_code_has_no_language() {
    assert_eq!(blocks("    a\n      b\n"), vec![code_block(None, "a\n  b")]);
}

#[test]
fn blockquotes_nest() {
    assert_eq!(
        blocks("> a\n>\n> > b"),
        vec![Block::Blockquote(vec![
            para(vec![text("a")]),
            Block::Blockquote(vec![para(vec![text("b")])]),
        ])]
    );
}

#[test]
fn gfm_alerts_become_quotes_with_a_bold_label() {
    let md = "> [!WARNING]\n> Back up first.";
    assert_eq!(
        blocks(md),
        vec![Block::Blockquote(vec![
            para(vec![strong("Warning")]),
            para(vec![text("Back up first.")]),
        ])]
    );
    assert_eq!(notes(md).count(NoteCode::CalloutsAsQuotes), 1);
}

#[test]
fn obsidian_callouts_use_their_title_or_type() {
    assert_eq!(
        blocks("> [!tip]- Remember this\n> Labels help."),
        vec![Block::Blockquote(vec![
            para(vec![strong("Remember this")]),
            para(vec![text("Labels help.")]),
        ])]
    );
    assert_eq!(
        blocks("> [!faq]\n> Answer."),
        vec![Block::Blockquote(vec![
            para(vec![strong("Faq")]),
            para(vec![text("Answer.")]),
        ])]
    );
}

#[test]
fn a_quote_that_only_looks_like_a_callout_stays_a_quote() {
    let md = "> [!not a callout] text";
    assert_eq!(
        blocks(md),
        vec![Block::Blockquote(vec![para(vec![text(
            "[!not a callout] text"
        )])])]
    );
    assert_eq!(notes(md).count(NoteCode::CalloutsAsQuotes), 0);
}

#[test]
fn nested_bullet_and_ordered_lists() {
    assert_eq!(
        blocks("3. a\n   - b\n4. c"),
        vec![Block::OrderedList {
            order: 3,
            items: vec![
                item(vec![
                    para(vec![text("a")]),
                    Block::BulletList(vec![item(vec![para(vec![text("b")])])]),
                ]),
                item(vec![para(vec![text("c")])]),
            ],
        }]
    );
}

#[test]
fn loose_list_items_hold_paragraphs() {
    assert_eq!(
        blocks("- a\n\n  more\n\n- b"),
        vec![Block::BulletList(vec![
            item(vec![para(vec![text("a")]), para(vec![text("more")])]),
            item(vec![para(vec![text("b")])]),
        ])]
    );
}

#[test]
fn task_lists_become_bullet_lists_with_boxes() {
    let md = "- [ ] open\n- [x] done\n\n1. [ ] numbered";
    assert_eq!(
        blocks(md),
        vec![
            Block::BulletList(vec![
                item(vec![para(vec![text("\u{2610} open")])]),
                item(vec![para(vec![text("\u{2611} done")])]),
            ]),
            Block::OrderedList {
                order: 1,
                items: vec![item(vec![para(vec![text("\u{2610} numbered")])])],
            },
        ]
    );
    assert_eq!(notes(md).count(NoteCode::ChecklistsAsBullets), 2);
}

fn th(inlines: Vec<Inline>) -> TableCell {
    TableCell::new(true, inlines)
}

fn td(inlines: Vec<Inline>) -> TableCell {
    TableCell::new(false, inlines)
}

fn aligned(mut cell: TableCell, align: Alignment) -> TableCell {
    cell.align = Some(align);
    cell
}

fn table(rows: Vec<Vec<TableCell>>) -> Block {
    Block::Table(Table { rows })
}

#[test]
fn gfm_tables_parse_into_the_table_node() {
    let parsed = super::parse::parse("| A | B | C |\n|:--|--:|---|\n| **x** | y |\n| z |\n");
    assert_eq!(
        parsed.document.blocks,
        vec![table(vec![
            vec![
                aligned(th(vec![text("A")]), Alignment::Left),
                aligned(th(vec![text("B")]), Alignment::Right),
                th(vec![text("C")]),
            ],
            vec![
                aligned(td(vec![strong("x")]), Alignment::Left),
                aligned(td(vec![text("y")]), Alignment::Right),
                td(vec![]),
            ],
            vec![
                aligned(td(vec![text("z")]), Alignment::Left),
                aligned(td(vec![]), Alignment::Right),
                td(vec![]),
            ],
        ])]
    );
}

#[test]
fn gfm_tables_become_table_nodes() {
    let md = "| Name | Count | Note |\n|:-----|------:|:----:|\n| **Alpha** | 1 | a \\| pipe<br>next |\n| Beta | 22 | `a\\|b` |\n";
    let cell = |tag: &str, align: &str, content: Value| {
        json!({"type": tag, "attrs": {"colspan": 1, "rowspan": 1, "align": align}, "content": [
            {"type": "paragraph", "content": content}
        ]})
    };
    let plain = |s: &str| json!([{"type": "text", "text": s}]);
    assert_eq!(
        json(md),
        json!({"type": "doc", "content": [{"type": "table", "content": [
            {"type": "table_row", "content": [
                cell("table_header", "left", plain("Name")),
                cell("table_header", "right", plain("Count")),
                cell("table_header", "center", plain("Note")),
            ]},
            {"type": "table_row", "content": [
                cell("table_cell", "left", json!([
                    {"type": "text", "text": "Alpha", "marks": [{"type": "strong", "attrs": {}}]}
                ])),
                cell("table_cell", "right", plain("1")),
                cell("table_cell", "center", json!([
                    {"type": "text", "text": "a | pipe"},
                    {"type": "hard_break"},
                    {"type": "text", "text": "next"}
                ])),
            ]},
            {"type": "table_row", "content": [
                cell("table_cell", "left", plain("Beta")),
                cell("table_cell", "right", plain("22")),
                cell("table_cell", "center", json!([
                    {"type": "text", "text": "a|b", "marks": [{"type": "code", "attrs": {}}]}
                ])),
            ]},
        ]}]})
    );
    assert!(notes(md).is_empty(), "{:?}", notes(md));
}

#[test]
fn unaligned_columns_carry_no_align_and_empty_cells_an_empty_paragraph() {
    assert_eq!(
        json("| A |  |\n|---|---|\n"),
        json!({"type": "doc", "content": [{"type": "table", "content": [
            {"type": "table_row", "content": [
                {"type": "table_header", "attrs": {"colspan": 1, "rowspan": 1}, "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "A"}]}
                ]},
                {"type": "table_header", "attrs": {"colspan": 1, "rowspan": 1}, "content": [
                    {"type": "paragraph"}
                ]},
            ]}
        ]}]})
    );
}

#[test]
fn tables_in_lists_and_quotes_move_after_their_top_level_block() {
    let md = "- one\n\n  | A |\n  |---|\n  | 1 |\n- two\n\n> quoted\n>\n> | B |\n> |---|\n\nafter";
    let got = blocks(md);
    let kinds: Vec<&str> = got
        .iter()
        .map(|b| match b {
            Block::BulletList(_) => "list",
            Block::Table(_) => "table",
            Block::Blockquote(_) => "quote",
            Block::Paragraph(_) => "paragraph",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["list", "table", "quote", "table", "paragraph"]);
    let Block::BulletList(items) = &got[0] else {
        panic!("list")
    };
    assert_eq!(
        items,
        &vec![
            item(vec![para(vec![text("one")])]),
            item(vec![para(vec![text("two")])])
        ]
    );
    assert_eq!(notes(md).count(NoteCode::TablesHoisted), 2);
}

#[test]
fn a_list_item_holding_only_a_table_keeps_an_empty_paragraph() {
    let (got, notes) = normalised(vec![Block::BulletList(vec![item(vec![table(vec![
        vec![td(vec![text("x")])],
    ])])])]);
    assert_eq!(
        got,
        vec![
            Block::BulletList(vec![item(vec![para(vec![])])]),
            table(vec![vec![td(vec![text("x")])]]),
        ]
    );
    assert_eq!(notes.count(NoteCode::TablesHoisted), 1);
}

#[test]
fn ragged_rows_are_padded_with_empty_cells() {
    let (got, notes) = normalised(vec![table(vec![
        vec![th(vec![text("A")])],
        vec![
            td(vec![text("1")]),
            td(vec![text("2")]),
            td(vec![text("3")]),
        ],
        vec![],
    ])]);
    assert_eq!(
        got,
        vec![table(vec![
            vec![th(vec![text("A")]), th(vec![]), th(vec![])],
            vec![
                td(vec![text("1")]),
                td(vec![text("2")]),
                td(vec![text("3")])
            ],
            vec![td(vec![]), td(vec![]), td(vec![])],
        ])]
    );
    assert_eq!(notes.count(NoteCode::TableRowsPadded), 1);
}

#[test]
fn spans_are_trimmed_to_the_table_and_count_as_columns() {
    let span = |colspan: u32, rowspan: u32| TableCell {
        colspan,
        rowspan,
        colwidth: Some(vec![100; colspan as usize]),
        ..td(vec![text("s")])
    };
    let (got, notes) = normalised(vec![table(vec![
        vec![span(1, 2), td(vec![text("b")])],
        vec![span(3, 5)],
    ])]);
    // The 1x2 cell holds column 0 of the second row, so the 3x5 cell starts
    // at column 1 and is cut to the one row left; the first row is padded
    // to the four columns that makes.
    let trimmed = TableCell {
        rowspan: 1,
        ..span(3, 5)
    };
    assert_eq!(
        got,
        vec![table(vec![
            vec![span(1, 2), td(vec![text("b")]), td(vec![]), td(vec![])],
            vec![trimmed],
        ])]
    );
    assert_eq!(notes.count(NoteCode::TableRowsPadded), 1);
}

#[test]
fn a_table_with_no_cells_is_dropped() {
    let (got, notes) = normalised(vec![table(vec![]), table(vec![vec![], vec![]])]);
    assert_eq!(got, vec![para(vec![])]);
    assert!(notes.is_empty());
}

#[test]
fn cells_hold_only_cell_blocks() {
    let cell = |blocks: Vec<Block>| TableCell {
        blocks,
        ..td(vec![])
    };
    let (got, notes) = normalised(vec![table(vec![vec![
        cell(vec![
            Block::Heading {
                level: 2,
                content: vec![text("Head "), code("c")],
            },
            Block::Blockquote(vec![para(vec![text("quoted")])]),
            Block::HorizontalRule,
            Block::EmbeddedDocument {
                document_uuid: "d1".into(),
                document_title: "Child".into(),
            },
            table(vec![
                vec![td(vec![text("a")]), td(vec![text("b")])],
                vec![td(vec![text("c")]), td(vec![])],
            ]),
            Block::BulletList(vec![item(vec![
                para(vec![text("li")]),
                table(vec![vec![td(vec![text("deep")])]]),
            ])]),
            code_block(None, "x"),
        ]),
        cell(vec![para(vec![])]),
    ]])]);
    assert_eq!(
        got,
        vec![table(vec![vec![
            cell(vec![
                para(vec![strong("Head "), code("c")]),
                para(vec![text("quoted")]),
                para(vec![text("Child")]),
                para(vec![text("a · b")]),
                para(vec![text("c")]),
                Block::BulletList(vec![item(vec![
                    para(vec![text("li")]),
                    para(vec![text("deep")]),
                ])]),
                code_block(None, "x"),
            ]),
            cell(vec![para(vec![])]),
        ]])]
    );
    // Heading, quote, rule, embed, and the two tables.
    assert_eq!(notes.count(NoteCode::TableCellsFlattened), 6);
    assert_eq!(notes.count(NoteCode::TablesHoisted), 0);
}

#[test]
fn strikethrough_and_html_formatting_are_removed() {
    let md = "a ~~b~~ <u>c</u> <mark>d</mark>";
    assert_eq!(blocks(md), vec![para(vec![text("a b c d")])]);
    assert_eq!(notes(md).count(NoteCode::StrikethroughRemoved), 3);
}

#[test]
fn footnotes_are_numbered_and_listed_at_the_end() {
    let md = "A[^z] and B[^a].\n\n[^a]: First defined.\n[^z]: Second defined.\n";
    assert_eq!(
        blocks(md),
        vec![
            para(vec![text("A[1] and B[2].")]),
            Block::Heading {
                level: 2,
                content: vec![text("Notes")],
            },
            Block::OrderedList {
                order: 1,
                items: vec![
                    item(vec![para(vec![text("Second defined.")])]),
                    item(vec![para(vec![text("First defined.")])]),
                ],
            },
        ]
    );
}

#[test]
fn math_becomes_code() {
    let md = "Inline $x^2$ then\n\n$$\na+b\n$$\n\nafter";
    let got = blocks(md);
    assert_eq!(
        got[0],
        para(vec![text("Inline "), code("x^2"), text(" then")])
    );
    assert!(got.contains(&code_block(Some("latex"), "a+b")), "{got:?}");
    assert_eq!(got.last(), Some(&para(vec![text("after")])));
    assert_eq!(notes(md).count(NoteCode::FormulasAsCode), 2);
}

#[test]
fn definition_lists_become_bold_terms_and_paragraphs() {
    assert_eq!(
        blocks("Term\n: Its definition.\n"),
        vec![
            para(vec![strong("Term")]),
            para(vec![text("Its definition.")])
        ]
    );
}

#[test]
fn thematic_breaks_become_horizontal_rules() {
    assert_eq!(
        blocks("a\n\n---\n\nb\n\n***"),
        vec![
            para(vec![text("a")]),
            Block::HorizontalRule,
            para(vec![text("b")]),
            Block::HorizontalRule,
        ]
    );
}

#[test]
fn html_blocks_keep_their_text_without_scripts() {
    let md = "<div class=\"box\">\n  <p>Kept &amp; flattened</p>\n  <script>alert('x')</script>\n</div>\n";
    assert_eq!(blocks(md), vec![para(vec![text("Kept & flattened")])]);
    let n = notes(md);
    assert_eq!(n.count(NoteCode::HtmlRemoved), 1);
    assert_eq!(n.count(NoteCode::HtmlAsText), 1);
}

#[test]
fn inline_html_maps_formatting_and_drops_the_rest() {
    let md = "<b>b</b> <i>i</i> <code>c</code> <span>s</span> <!-- x -->end";
    assert_eq!(
        blocks(md),
        vec![para(vec![
            strong("b"),
            text(" "),
            em("i"),
            text(" "),
            code("c"),
            text(" s end"),
        ])]
    );
    let n = notes(md);
    assert_eq!(n.count(NoteCode::HtmlAsText), 1);
    assert_eq!(n.count(NoteCode::HtmlRemoved), 1);
}

#[test]
fn front_matter_is_metadata_not_body() {
    let converted = convert("---\ntitle: X\n---\n\nBody");
    assert_eq!(converted.document.blocks, vec![para(vec![text("Body")])]);
    let front = converted.front_matter.expect("front matter");
    assert_eq!(front.kind, FrontMatterKind::Yaml);
    assert_eq!(front.raw.trim(), "title: X");

    let toml = convert("+++\ntitle = \"X\"\n+++\n\nBody");
    assert_eq!(
        toml.front_matter.map(|f| f.kind),
        Some(FrontMatterKind::Pluses)
    );
}

#[test]
fn a_rule_pair_later_in_the_page_is_not_front_matter() {
    let converted = convert("Intro\n\n---\n\nmiddle\n\n---\n\nend");
    assert!(converted.front_matter.is_none());
    assert_eq!(converted.document.blocks.len(), 5);
}

// Content rules.

#[test]
fn an_empty_document_gets_one_paragraph() {
    for md in [
        "",
        "   \n\n",
        "<!-- only a comment -->",
        "[^x]: unused footnote? no",
    ] {
        let got = blocks(md);
        assert!(!got.is_empty(), "{md:?}");
    }
    assert_eq!(blocks(""), vec![para(vec![])]);
}

#[test]
fn a_list_item_starts_with_a_paragraph() {
    assert_eq!(
        blocks("- ```\n  code\n  ```\n- - nested\n-\n- > quote"),
        vec![Block::BulletList(vec![
            item(vec![para(vec![]), code_block(None, "code")]),
            item(vec![
                para(vec![]),
                Block::BulletList(vec![item(vec![para(vec![text("nested")])])]),
            ]),
            item(vec![para(vec![])]),
            item(vec![
                para(vec![]),
                Block::Blockquote(vec![para(vec![text("quote")])]),
            ]),
        ])]
    );
}

#[test]
fn an_empty_blockquote_is_dropped() {
    assert_eq!(blocks(">\n\nafter"), vec![para(vec![text("after")])]);
}

#[test]
fn an_empty_list_is_dropped() {
    let (got, _) = normalised(vec![
        Block::BulletList(vec![]),
        Block::OrderedList {
            order: 2,
            items: vec![],
        },
    ]);
    assert_eq!(got, vec![para(vec![])]);
}

#[test]
fn headings_hold_inline_content_including_images() {
    assert_eq!(
        blocks("# Logo ![l](https://e.com/l.png)"),
        vec![Block::Heading {
            level: 1,
            content: vec![
                text("Logo "),
                Inline::Image(Image {
                    src: "https://e.com/l.png".into(),
                    alt: Some("l".into()),
                    ..Image::default()
                }),
            ],
        }]
    );
    let (got, _) = normalised(vec![Block::Heading {
        level: 9,
        content: vec![],
    }]);
    assert_eq!(
        got,
        vec![Block::Heading {
            level: 6,
            content: vec![]
        }]
    );
}

fn quote_depth(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Blockquote(inner) => 1 + quote_depth(inner),
            Block::BulletList(items) => {
                1 + items
                    .iter()
                    .map(|i| quote_depth(&i.blocks))
                    .max()
                    .unwrap_or(0)
            }
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn nesting_deeper_than_the_limit_flattens_into_the_last_level() {
    let md = format!("{} deep", "> ".repeat(MAX_DEPTH + 4));
    let got = blocks(&md);
    assert_eq!(quote_depth(&got), MAX_DEPTH);
    assert_eq!(ir_text(&got), "deep");
    assert_eq!(notes(&md).count(NoteCode::NestingFlattened), 1);

    let lists: String = (0..MAX_DEPTH + 3)
        .map(|i| format!("{}- level {i}\n", "  ".repeat(i)))
        .collect();
    let got = blocks(&lists);
    assert_eq!(quote_depth(&got), MAX_DEPTH);
    assert!(ir_text(&got).contains(&format!("level {}", MAX_DEPTH + 2)));
}

fn ir_text(blocks: &[Block]) -> String {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(i) | Block::Heading { content: i, .. } => {
                out.push(super::ir::plain_text(i))
            }
            Block::Blockquote(inner) => out.push(ir_text(inner)),
            Block::BulletList(items) | Block::OrderedList { items, .. } => {
                for item in items {
                    out.push(ir_text(&item.blocks));
                }
            }
            Block::CodeBlock { text, .. } => out.push(text.clone()),
            _ => {}
        }
    }
    out.retain(|s| !s.is_empty());
    out.join(" ")
}

#[test]
fn empty_runs_are_dropped_and_equal_runs_merged() {
    let (got, _) = normalised(vec![para(vec![
        text(""),
        text("a"),
        text("b"),
        strong(""),
        strong("c"),
        Inline::Text(Text {
            text: "d\ne".into(),
            marks: Marks::strong(),
        }),
    ])]);
    assert_eq!(got, vec![para(vec![text("ab"), strong("cd e")])]);
}

#[test]
fn required_attrs_are_present_or_the_node_becomes_text() {
    let (got, notes) = normalised(vec![
        para(vec![
            Inline::Image(Image::default()),
            Inline::Mention {
                uuid: String::new(),
                name: "Sam".into(),
                avatar_url: None,
            },
            text(" "),
            Inline::TicketLink {
                ticket_id: "7".into(),
                href: String::new(),
            },
        ]),
        Block::EmbeddedDocument {
            document_uuid: String::new(),
            document_title: "Child".into(),
        },
        Block::EmbeddedDocument {
            document_uuid: "u".into(),
            document_title: String::new(),
        },
    ]);
    assert_eq!(
        got,
        vec![
            para(vec![text("@Sam 7")]),
            para(vec![text("Child")]),
            Block::EmbeddedDocument {
                document_uuid: "u".into(),
                document_title: "Untitled".into(),
            },
        ]
    );
    assert_eq!(notes.count(NoteCode::ImagesDropped), 1);
}

#[test]
fn normalising_twice_changes_nothing() {
    for md in fixture_inputs().into_iter().map(|(_, md)| md) {
        let once = convert(&md);
        let mut notes = Notes::default();
        let twice = normalise(once.document.clone(), &DefaultResolver, &mut notes);
        assert_eq!(twice, once.document);
    }
}

// Encoder conventions.

#[test]
fn encoder_writes_typed_attrs_and_y_prosemirror_marks() {
    let got = json("## Title\n\n1. a\n\n**b** *c* `d` [e](https://e.com) plain");
    assert_eq!(
        got,
        json!({"type": "doc", "content": [
            {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Title"}]},
            {"type": "ordered_list", "attrs": {"order": 1}, "content": [
                {"type": "list_item", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "a"}]}
                ]}
            ]},
            {"type": "paragraph", "content": [
                {"type": "text", "text": "b", "marks": [{"type": "strong", "attrs": {}}]},
                {"type": "text", "text": " "},
                {"type": "text", "text": "c", "marks": [{"type": "em", "attrs": {}}]},
                {"type": "text", "text": " "},
                {"type": "text", "text": "d", "marks": [{"type": "code", "attrs": {}}]},
                {"type": "text", "text": " "},
                {"type": "text", "text": "e", "marks": [{"type": "link", "attrs": {"href": "https://e.com", "title": null}}]},
                {"type": "text", "text": " plain"}
            ]}
        ]})
    );
}

#[test]
fn encoder_omits_null_attrs_and_empty_text() {
    let got = json("```\n```\n\n![](https://e.com/i.png)");
    assert_eq!(
        got,
        json!({"type": "doc", "content": [
            {"type": "code_block"},
            {"type": "paragraph", "content": [
                {"type": "image", "attrs": {"src": "https://e.com/i.png"}}
            ]}
        ]})
    );
    assert_eq!(
        json(""),
        json!({"type": "doc", "content": [{"type": "paragraph"}]})
    );
}

#[test]
fn encoder_writes_every_node_type() {
    let document = Document {
        blocks: vec![
            para(vec![
                Inline::Image(Image {
                    src: "/api/files/x".into(),
                    alt: None,
                    title: None,
                    width: Some(240),
                    align: Some(super::ir::ImageAlign::Center),
                }),
                Inline::Mention {
                    uuid: "u1".into(),
                    name: "Sam".into(),
                    avatar_url: None,
                },
                Inline::TicketLink {
                    ticket_id: "7".into(),
                    href: "/tickets/7".into(),
                },
            ]),
            Block::EmbeddedDocument {
                document_uuid: "d1".into(),
                document_title: "Child".into(),
            },
        ],
    };
    assert_eq!(
        decode::to_json(&encode(&document)),
        json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [
                {"type": "image", "attrs": {"src": "/api/files/x", "width": 240, "align": "center"}},
                {"type": "mention", "attrs": {"uuid": "u1", "name": "Sam"}},
                {"type": "ticket_link", "attrs": {"ticketId": "7", "href": "/tickets/7"}}
            ]},
            {"type": "embedded_document", "attrs": {"documentUuid": "d1", "documentTitle": "Child"}}
        ]})
    );
}

#[test]
fn encoder_writes_table_attrs_as_y_prosemirror_does() {
    use yrs::updates::decoder::Decode;
    use yrs::{Any, Doc, Out, Transact, Update, WriteTxn, Xml, XmlFragment, XmlOut};

    let cell = TableCell {
        colspan: 2,
        colwidth: Some(vec![120, 80]),
        align: Some(Alignment::Center),
        ..th(vec![text("wide")])
    };
    let document = Document {
        blocks: vec![table(vec![vec![cell], vec![td(vec![]), td(vec![])]])],
    };
    let bytes = encode(&document);
    assert_eq!(
        decode::to_json(&bytes),
        json!({"type": "doc", "content": [{"type": "table", "content": [
            {"type": "table_row", "content": [
                {"type": "table_header",
                 "attrs": {"colspan": 2, "rowspan": 1, "colwidth": [120, 80], "align": "center"},
                 "content": [{"type": "paragraph", "content": [{"type": "text", "text": "wide"}]}]}
            ]},
            {"type": "table_row", "content": [
                {"type": "table_cell", "attrs": {"colspan": 1, "rowspan": 1}, "content": [{"type": "paragraph"}]},
                {"type": "table_cell", "attrs": {"colspan": 1, "rowspan": 1}, "content": [{"type": "paragraph"}]}
            ]}
        ]}]})
    );

    // Numbers, not BigInts: y-prosemirror compares attrs with `!==`.
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(&bytes).expect("decodes"))
        .expect("applies");
    let fragment = txn.get_or_insert_xml_fragment("prosemirror");
    let Some(XmlOut::Element(table)) = fragment.get(&txn, 0) else {
        panic!("table");
    };
    assert!(table.attributes(&txn).next().is_none());
    let Some(XmlOut::Element(row)) = table.get(&txn, 0) else {
        panic!("row");
    };
    assert!(row.attributes(&txn).next().is_none());
    let Some(XmlOut::Element(header)) = row.get(&txn, 0) else {
        panic!("header");
    };
    assert_eq!(
        header.get_attribute(&txn, "colspan"),
        Some(Out::Any(Any::Number(2.0)))
    );
    assert_eq!(
        header.get_attribute(&txn, "rowspan"),
        Some(Out::Any(Any::Number(1.0)))
    );
    assert_eq!(
        header.get_attribute(&txn, "colwidth"),
        Some(Out::Any(Any::from(vec![
            Any::Number(120.0),
            Any::Number(80.0)
        ])))
    );
    assert_eq!(
        header.get_attribute(&txn, "align"),
        Some(Out::Any(Any::from("center")))
    );
    assert_eq!(header.get_attribute(&txn, "ychange"), None);
}

/// y-prosemirror keeps one XmlText per run of adjacent text nodes; two
/// adjacent XmlTexts trigger its merge-on-load path (issue #160) and change
/// the document on first open.
#[test]
fn encoder_writes_one_xml_text_per_text_run() {
    use yrs::updates::decoder::Decode;
    use yrs::{Doc, Transact, Update, WriteTxn, XmlFragment, XmlOut};

    let bytes = encode(&convert("a **b** c  \nd").document);
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(&bytes).expect("decodes"))
        .expect("applies");
    let fragment = txn.get_or_insert_xml_fragment("prosemirror");
    let Some(XmlOut::Element(paragraph)) = fragment.get(&txn, 0) else {
        panic!("paragraph");
    };
    let kinds: Vec<&str> = paragraph
        .children(&txn)
        .map(|c| match c {
            XmlOut::Text(_) => "text",
            XmlOut::Element(_) => "element",
            XmlOut::Fragment(_) => "fragment",
        })
        .collect();
    assert_eq!(kinds, vec!["text", "element", "text"]);
}

#[test]
fn markdown_to_yjs_returns_the_notes() {
    let (bytes, notes) = super::markdown_to_yjs("~~x~~");
    assert!(!bytes.is_empty());
    assert_eq!(notes.count(NoteCode::StrikethroughRemoved), 1);
}

// Golden fixtures. Each `tests/fixtures/docs_markdown/*.md`, and the welcome
// seed, is encoded to `*.yjs.json` (base64 of the v1 update). The frontend
// spec `docsMarkdownFixtures.spec.ts` loads each through y-prosemirror and
// checks it against the editor schema. Regenerate with
// `UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown`.

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/docs_markdown");

fn fixture_inputs() -> Vec<(String, String)> {
    let mut inputs: Vec<(String, String)> = std::fs::read_dir(FIXTURE_DIR)
        .expect("fixture dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .map(|p| {
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, std::fs::read_to_string(&p).expect("fixture reads"))
        })
        .collect();
    inputs.push((
        "getting-started".into(),
        include_str!("../../../seeds/getting-started.md").into(),
    ));
    inputs.sort();
    inputs
}

#[test]
fn golden_fixtures_are_current() {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::STANDARD;
    let regenerate = std::env::var_os("UPDATE_DOCS_MARKDOWN_FIXTURES").is_some();
    let mut stale = Vec::new();
    for (name, markdown) in fixture_inputs() {
        let converted = convert(&markdown);
        let fresh = encode_with_client_id(&converted.document, 1);
        let path = format!("{FIXTURE_DIR}/{name}.yjs.json");
        let stored = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let current = stored
            .as_ref()
            .and_then(|v| v["update"].as_str())
            .and_then(|b| engine.decode(b).ok());
        let notes_match = stored
            .as_ref()
            .is_some_and(|v| v["notes"] == json!(converted.notes));
        let matches = notes_match
            && current
                .as_deref()
                .is_some_and(|c| decode::to_json(c) == decode::to_json(&fresh));
        if matches {
            continue;
        }
        if regenerate {
            let body = json!({
                "source": format!("{name}.md"),
                "notes": converted.notes,
                "update": engine.encode(&fresh),
            });
            let text = serde_json::to_string_pretty(&body).expect("serialises") + "\n";
            std::fs::write(&path, text).expect("fixture writes");
        } else {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "stale docs_markdown fixtures {stale:?}; run UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown"
    );
}
