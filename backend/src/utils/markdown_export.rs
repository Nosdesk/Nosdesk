//! Nosdesk documents (y-prosemirror Yjs) to Markdown.
//!
//! The output is meant to come back through the importer
//! (`services::docs_markdown`) unchanged, so:
//!
//! - Inline text is read through the Yjs delta, and marks become Markdown
//!   (`**`, `*`, backticks, `[text](href)`). Where Markdown's delimiter rules
//!   can't hold a mark exactly (a bold run that ends in a space, a closing
//!   `**` between punctuation and a letter), that line's bold and italic are
//!   written as `<strong>` and `<em>`, which the importer reads as the same
//!   marks. Every inline line is parsed back to check which form holds.
//! - Text is escaped wherever a character could start Markdown syntax, and
//!   whitespace Markdown would strip at a line's edges is written as a
//!   character reference.
//! - Lists, quotes and code blocks nest by indentation, and two lists in a
//!   row alternate their markers so they stay two lists.
//! - Mentions, ticket links and embedded pages are written as the editor
//!   schema's own `parseDOM` elements (`span[data-mention]`,
//!   `span[data-ticket-link]`, `div[data-embedded-document]`), which other
//!   Markdown viewers show as their text.
//! - A table is a GFM pipe table when one holds it exactly, and an HTML
//!   `<table>` block otherwise. Column widths are never written.
//!
//! What Markdown can't hold at all: empty paragraphs (other than the first
//! in a list item), text that holds a line break, and bold or italic on code.

use std::collections::{HashMap, HashSet};
use std::panic;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use unic_langid::LanguageIdentifier;
use uuid::Uuid;
use yrs::types::text::YChange;
use yrs::types::Attrs;
use yrs::{
    updates::decoder::Decode, Any, Doc, Options, Out, ReadTxn, Text, Transact, Update, WriteTxn,
    Xml, XmlElementRef, XmlFragment, XmlOut, XmlTextRef,
};

use crate::db::DbConnection;
use crate::repository;
use crate::repository::PageAudience;
use crate::utils::i18n;

const MAX_EMBED_DEPTH: usize = 10;

/// The XmlFragment y-prosemirror binds the editor to.
const FRAGMENT: &str = "prosemirror";

/// Rewrites the destinations a document points at as it is written: page
/// links and image sources, for an export whose files sit side by side.
pub trait Links {
    /// What a link mark's `href` becomes.
    fn href(&self, href: &str) -> String {
        href.to_string()
    }
    /// What an image's `src` becomes.
    fn image_src(&self, src: &str) -> String {
        src.to_string()
    }
}

/// Destinations written as stored.
pub struct KeepLinks;

impl Links for KeepLinks {}

fn load(yjs_document: &[u8]) -> Option<Doc> {
    let doc = Doc::with_options(Options {
        skip_gc: true,
        ..Default::default()
    });
    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment(FRAGMENT);
    }
    let update = Update::decode_v1(yjs_document).ok()?;
    doc.transact_mut().apply_update(update).ok()?;
    Some(doc)
}

fn non_empty(markdown: String) -> Option<String> {
    let trimmed = markdown.trim_end();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// A Yjs document as Markdown, with nothing to read: embedded pages as their
/// element, ticket links named by id.
pub fn yjs_to_markdown(yjs_document: &[u8]) -> Option<String> {
    let doc = load(yjs_document)?;
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT)?;
    let numbers = TicketNumbers::new();
    let mut writer = Writer {
        txn: &txn,
        numbers: &numbers,
        links: &KeepLinks,
        embeds: Embeds::Element,
    };
    non_empty(writer.fragment(&fragment))
}

/// A Yjs document as Markdown for a file in an export: embedded pages as
/// their element, ticket links named by number (read in the connection's
/// pinned workspace), destinations through `links`.
pub fn yjs_to_markdown_for_export(
    yjs_document: &[u8],
    conn: &mut DbConnection,
    links: &dyn Links,
) -> Option<String> {
    let doc = load(yjs_document)?;
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT)?;
    let numbers = linked_ticket_numbers(&fragment, &txn, conn);
    let mut writer = Writer {
        txn: &txn,
        numbers: &numbers,
        links,
        embeds: Embeds::Element,
    };
    non_empty(writer.fragment(&fragment))
}

/// Everything needed to follow an `embedded_document` node: the connection to
/// read the embedded page with, and who the export is for.
///
/// They are bundled rather than passed separately because they must not be
/// separable. A page the caller may read can embed one they may not, so an
/// exporter holding a connection but no audience would silently render content
/// past the ACL. Requiring both to resolve an embed makes that a type error
/// rather than a discipline.
pub struct EmbedResolver<'a> {
    pub conn: &'a mut DbConnection,
    pub audience: PageAudience,
}

/// A Yjs document as Markdown, with each embedded page the audience may read
/// written in place.
pub fn yjs_to_markdown_with_embeds(
    yjs_document: &[u8],
    resolver: &mut EmbedResolver<'_>,
    visited: &mut HashSet<Uuid>,
    current_uuid: Option<Uuid>,
    depth: usize,
    locale: &LanguageIdentifier,
) -> Option<String> {
    if depth > MAX_EMBED_DEPTH {
        return Some(format!(
            "> {}\n",
            i18n::tr(locale, "markdown-embed-depth-limit")
        ));
    }

    if let Some(uuid) = current_uuid {
        if visited.contains(&uuid) {
            return Some(format!(
                "> {}\n",
                i18n::tr(locale, "markdown-embed-circular")
            ));
        }
        visited.insert(uuid);
    }

    let doc = load(yjs_document)?;
    let txn = doc.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT)?;
    let numbers = linked_ticket_numbers(&fragment, &txn, &mut *resolver.conn);
    let mut writer = Writer {
        txn: &txn,
        numbers: &numbers,
        links: &KeepLinks,
        embeds: Embeds::Resolve {
            resolver,
            visited,
            depth,
            locale,
        },
    };
    non_empty(writer.fragment(&fragment))
}

/// Ticket number by id, for the tickets a document links. A ticket missing
/// from it (deleted, or not in this workspace) is named by id.
type TicketNumbers = HashMap<i32, i32>;

/// The numbers of the tickets the document links, read in the connection's
/// pinned workspace. Empty when it links none or the read fails.
fn linked_ticket_numbers(
    fragment: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
    conn: &mut DbConnection,
) -> TicketNumbers {
    fn collect(node: &XmlOut, txn: &yrs::Transaction, ids: &mut Vec<i32>) {
        match node {
            XmlOut::Element(elem) => {
                if elem.tag().as_ref() == "ticket_link" {
                    let id = attr_string(elem, txn, "ticketId").and_then(|v| v.parse::<i32>().ok());
                    ids.extend(id);
                }
                for child in elem.children(txn) {
                    collect(&child, txn, ids);
                }
            }
            XmlOut::Fragment(frag) => {
                for child in frag.children(txn) {
                    collect(&child, txn, ids);
                }
            }
            XmlOut::Text(_) => {}
        }
    }
    let mut ids = Vec::new();
    for child in fragment.children(txn) {
        collect(&child, txn, &mut ids);
    }
    if ids.is_empty() {
        return TicketNumbers::new();
    }
    repository::tickets::numbers_in_pinned_workspace(conn, &ids).unwrap_or_default()
}

// Attributes.

fn any_string(value: &Any) -> Option<String> {
    match value {
        Any::Null | Any::Undefined => None,
        Any::String(s) => Some(s.to_string()),
        Any::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            Some(format!("{}", *n as i64))
        }
        Any::Number(n) => Some(n.to_string()),
        Any::BigInt(n) => Some(n.to_string()),
        Any::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn attr_string<T: ReadTxn>(elem: &XmlElementRef, txn: &T, name: &str) -> Option<String> {
    match elem.get_attribute(txn, name)? {
        Out::Any(value) => any_string(&value),
        other => Some(other.to_string(txn)),
    }
}

/// A whole-number attr (`level`, `order`, `colspan`), when it holds one.
/// Older writers stored `level` as a string.
fn attr_count<T: ReadTxn>(elem: &XmlElementRef, txn: &T, name: &str) -> Option<u64> {
    let n = match elem.get_attribute(txn, name)? {
        Out::Any(Any::Number(n)) => n,
        Out::Any(Any::BigInt(n)) => n as f64,
        Out::Any(Any::String(s)) => s.trim().parse().ok()?,
        _ => return None,
    };
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < 1e15).then_some(n as u64)
}

// Inline content.

/// A text run's marks. `ychange` is the editor's own and never read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MarkSet {
    strong: bool,
    em: bool,
    code: bool,
    link: Option<LinkMark>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkMark {
    href: String,
    title: Option<String>,
}

/// One inline node, with destinations already rewritten and ticket labels
/// already chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Text(String, MarkSet),
    Break,
    Image {
        src: String,
        alt: Option<String>,
        title: Option<String>,
        width: Option<String>,
        align: Option<String>,
    },
    Mention {
        uuid: String,
        name: String,
        avatar_url: Option<String>,
    },
    Ticket {
        ticket_id: String,
        href: String,
        label: String,
    },
}

/// Marks from a delta insert's attributes. y-prosemirror names a mark that
/// may overlap itself `name--hash`; a mark set to null is absent. Older
/// writers stored boolean marks as `true` rather than `{}`.
fn marks_from(attrs: Option<&Attrs>) -> MarkSet {
    let mut marks = MarkSet::default();
    let Some(attrs) = attrs else {
        return marks;
    };
    for (key, value) in attrs.iter() {
        if matches!(value, Any::Null | Any::Undefined | Any::Bool(false)) {
            continue;
        }
        let name = key.split("--").next().unwrap_or_default();
        match name {
            "strong" => marks.strong = true,
            "em" => marks.em = true,
            "code" => marks.code = true,
            "link" => {
                if let Any::Map(map) = value {
                    let href = map.get("href").and_then(any_string).unwrap_or_default();
                    let title = map
                        .get("title")
                        .and_then(any_string)
                        .filter(|t| !t.is_empty());
                    marks.link = Some(LinkMark { href, title });
                }
            }
            _ => {}
        }
    }
    marks
}

/// Where inline content sits: what a hard break is written as, and which
/// characters are special.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// A paragraph: block syntax can start any of its lines.
    Block,
    /// A heading: one line; a `#` run at its end would close it.
    Heading,
    /// A GFM table cell: one line, trimmed, split at `|`.
    Cell,
    /// Mid-line inside another construct (link text, alt text, a label).
    Inside,
}

/// What the writer does with an `embedded_document`.
enum Embeds<'a, 'r> {
    /// Writes the schema's element, so an import restores the embed.
    Element,
    /// Writes the embedded page's content in place, when the audience may
    /// read it.
    Resolve {
        resolver: &'a mut EmbedResolver<'r>,
        visited: &'a mut HashSet<Uuid>,
        depth: usize,
        locale: &'a LanguageIdentifier,
    },
}

struct Writer<'a, 'r, T: ReadTxn> {
    txn: &'a T,
    numbers: &'a TicketNumbers,
    links: &'a dyn Links,
    embeds: Embeds<'a, 'r>,
}

/// Where a run of blocks sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    /// The document itself.
    Top,
    /// A quote.
    Quote,
    /// A list item: its first paragraph is kept even when empty, and a list
    /// right after a paragraph stays tight.
    Item,
}

/// What the previous block written was, for the separator and the list
/// marker of the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Written {
    Paragraph,
    EmptyParagraph,
    /// A list, and whether it used the alternate marker.
    List {
        ordered: bool,
        alternate: bool,
    },
    Other,
}

impl<T: ReadTxn> Writer<'_, '_, T> {
    fn fragment(&mut self, fragment: &yrs::XmlFragmentRef) -> String {
        let children: Vec<XmlOut> = fragment.children(self.txn).collect();
        self.blocks(&children, Container::Top)
    }

    /// Block children, separated by blank lines (or one line break where a
    /// list item keeps a nested list tight).
    fn blocks(&mut self, children: &[XmlOut], container: Container) -> String {
        let mut out = String::new();
        let mut previous: Option<Written> = None;
        let mut previous_text = String::new();
        for (index, child) in children.iter().enumerate() {
            let (text, written) = match child {
                XmlOut::Element(elem) => self.block(elem, previous),
                XmlOut::Text(text) => {
                    let items = self.text_items(text, &MarkSet::default());
                    (render_inline(&items, Ctx::Block), Written::Paragraph)
                }
                XmlOut::Fragment(fragment) => {
                    let inner: Vec<XmlOut> = fragment.children(self.txn).collect();
                    (self.blocks(&inner, container), Written::Other)
                }
            };
            let written = match written {
                Written::Paragraph if text.is_empty() => Written::EmptyParagraph,
                other => other,
            };
            if text.is_empty() && !(container == Container::Item && index == 0) {
                continue;
            }
            if let Some(before) = previous {
                // An empty first item can't interrupt a paragraph: its
                // marker would read as a setext underline.
                let starts_empty = !text.split('\n').next().unwrap_or_default().contains(' ');
                // A paragraph line that reads as an HTML block (`<br>` or an
                // `<img>` alone) runs on to the next blank line, so a list
                // after it can't follow tight.
                let tight = container == Container::Item
                    && (before == Written::EmptyParagraph
                        || (before == Written::Paragraph
                            && !starts_empty
                            && !opens_html_block(&previous_text)
                            && matches!(child, XmlOut::Element(e) if self.is_tight_list(e))));
                out.push_str(if tight { "\n" } else { "\n\n" });
            }
            out.push_str(&text);
            previous = Some(written);
            previous_text = text;
        }
        out
    }

    /// A list that may follow a paragraph line directly: a bullet list, or an
    /// ordered list starting at 1 (no other start can interrupt one).
    fn is_tight_list(&self, elem: &XmlElementRef) -> bool {
        match elem.tag().as_ref() {
            "bullet_list" => true,
            "ordered_list" => attr_count(elem, self.txn, "order").unwrap_or(1) == 1,
            _ => false,
        }
    }

    fn block(&mut self, elem: &XmlElementRef, previous: Option<Written>) -> (String, Written) {
        let tag = elem.tag().to_string();
        match tag.as_str() {
            "paragraph" => {
                let items = self.element_items(elem);
                (render_inline(&items, Ctx::Block), Written::Paragraph)
            }
            "heading" => {
                let level = attr_count(elem, self.txn, "level").unwrap_or(1).clamp(1, 6) as usize;
                let items = self.element_items(elem);
                let content = render_inline(&items, Ctx::Heading);
                let hashes = "#".repeat(level);
                let text = if content.is_empty() {
                    hashes
                } else {
                    format!("{hashes} {content}")
                };
                (text, Written::Other)
            }
            "code_block" => {
                let language = attr_string(elem, self.txn, "language").unwrap_or_default();
                (code_fence(&language, &self.raw_text(elem)), Written::Other)
            }
            "blockquote" => {
                let children: Vec<XmlOut> = elem.children(self.txn).collect();
                let inner = self.blocks(&children, Container::Quote);
                (quote(&inner), Written::Other)
            }
            "bullet_list" | "ordered_list" => {
                let ordered = tag == "ordered_list";
                let alternate = matches!(
                    previous,
                    Some(Written::List { ordered: o, alternate: false }) if o == ordered
                );
                (
                    self.list(elem, ordered, alternate),
                    Written::List { ordered, alternate },
                )
            }
            // Not `---`: the parser can read a `---` line after a blank
            // line as the start of a YAML front matter block.
            "horizontal_rule" => ("***".to_string(), Written::Other),
            "embedded_document" => (self.embedded_document(elem), Written::Other),
            "table" => (self.table(elem), Written::Other),
            "image" | "mention" | "ticket_link" | "hard_break" => {
                // An inline node where a block belongs: a paragraph holding it.
                let items = self.items_of(std::slice::from_ref(&XmlOut::Element(elem.clone())));
                (render_inline(&items, Ctx::Block), Written::Paragraph)
            }
            _ => {
                // A node this writer doesn't know: its inline content.
                let items = self.element_items(elem);
                (render_inline(&items, Ctx::Block), Written::Paragraph)
            }
        }
    }

    fn list(&mut self, elem: &XmlElementRef, ordered: bool, alternate: bool) -> String {
        let mut number = attr_count(elem, self.txn, "order").unwrap_or(1);
        let mut items = Vec::new();
        for child in elem.children(self.txn) {
            let XmlOut::Element(li) = child else {
                continue;
            };
            if li.tag().as_ref() != "list_item" {
                continue;
            }
            let marker = if ordered {
                let marker = format!("{number}{}", if alternate { ')' } else { '.' });
                number += 1;
                marker
            } else {
                (if alternate { "*" } else { "-" }).to_string()
            };
            let children: Vec<XmlOut> = li.children(self.txn).collect();
            let body = self.blocks(&children, Container::Item);
            items.push(list_item(&marker, &body));
        }
        let loose = items.iter().any(|item| item.contains("\n\n"));
        if loose {
            // pulldown-cmark closes the list's parent too after a loose
            // list's empty last item (seen inside a quote in a list item).
            // An empty comment keeps the item open and imports as nothing.
            if let Some(last) = items.last_mut() {
                if !last.contains(' ') && !last.contains('\n') {
                    last.push_str(" <!-- -->");
                }
            }
        }
        items.join(if loose { "\n\n" } else { "\n" })
    }

    fn embedded_document(&mut self, elem: &XmlElementRef) -> String {
        let uuid = attr_string(elem, self.txn, "documentUuid").unwrap_or_default();
        let title = attr_string(elem, self.txn, "documentTitle");
        let Embeds::Resolve {
            resolver,
            visited,
            depth,
            locale,
        } = &mut self.embeds
        else {
            return embed_element(&uuid, &title.unwrap_or_else(|| "Untitled".to_string()));
        };
        let locale: &LanguageIdentifier = locale;
        let depth = *depth;
        let title = title.unwrap_or_else(|| i18n::tr(locale, "docs-untitled-page"));

        if let Ok(uuid) = Uuid::parse_str(&uuid) {
            if depth < MAX_EMBED_DEPTH && !visited.contains(&uuid) {
                // An embedded page carries its own ACL. Read it only if this
                // export's audience may read it; otherwise fall through to the
                // reference below, which prints the title the embedding
                // document already contains and none of the embedded content.
                let readable = repository::get_documentation_page_by_uuid(&uuid, resolver.conn)
                    .ok()
                    .filter(|page| resolver.audience.can_read(resolver.conn, page.id));
                if let Some(page) = readable {
                    let conn = &mut *resolver.conn;
                    let yjs_doc = page.yjs_document.as_ref().cloned().or_else(|| {
                        repository::documentation_page_tickets::most_recent_resolves_ticket_id(
                            conn, page.id,
                        )
                        .ok()
                        .flatten()
                        .and_then(|tid| {
                            repository::get_article_content_by_ticket_id(conn, tid).ok()
                        })
                        .and_then(|a| a.yjs_document)
                    });
                    if let Some(doc_bytes) = yjs_doc {
                        if let Some(content) = yjs_to_markdown_with_embeds(
                            &doc_bytes,
                            resolver,
                            visited,
                            Some(uuid),
                            depth + 1,
                            locale,
                        ) {
                            let header = i18n::tr_with(
                                locale,
                                "markdown-embed-reference",
                                &[("title", title.clone().into())],
                            );
                            let bold = MarkSet {
                                strong: true,
                                ..MarkSet::default()
                            };
                            let header = render_inline(&[Item::Text(header, bold)], Ctx::Block);
                            return format!("***\n\n{header}\n\n{}\n\n***", content.trim_end());
                        }
                    }
                }
            }
        }

        let reference = i18n::tr_with(
            locale,
            "markdown-embed-reference-fallback",
            &[("title", title.into())],
        );
        render_inline(&[Item::Text(reference, MarkSet::default())], Ctx::Block)
    }

    // Tables.

    fn table(&mut self, elem: &XmlElementRef) -> String {
        let rows: Vec<Vec<XmlElementRef>> = elem
            .children(self.txn)
            .filter_map(|row| match row {
                XmlOut::Element(row) if row.tag().as_ref() == "table_row" => Some(row),
                _ => None,
            })
            .map(|row| {
                row.children(self.txn)
                    .filter_map(|cell| match cell {
                        XmlOut::Element(cell)
                            if matches!(cell.tag().as_ref(), "table_cell" | "table_header") =>
                        {
                            Some(cell)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        match self.pipe_table_shape(&rows) {
            Some(shape) => self.pipe_table(&shape),
            None => self.html_table(&rows),
        }
    }

    /// The column alignments and each cell's paragraph, when a GFM pipe table
    /// holds this table exactly: a first row of header cells and no header
    /// cell after it, every row as wide as the first, no spans, one paragraph
    /// per cell, and one alignment per column.
    fn pipe_table_shape(&self, rows: &[Vec<XmlElementRef>]) -> Option<PipeTable> {
        let columns = rows.first()?.len();
        if columns == 0 {
            return None;
        }
        let mut alignments: Vec<Option<Option<String>>> = vec![None; columns];
        let mut paragraphs = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            if row.len() != columns {
                return None;
            }
            let mut row_paragraphs = Vec::with_capacity(columns);
            for (c, cell) in row.iter().enumerate() {
                let header = cell.tag().as_ref() == "table_header";
                if header != (r == 0) {
                    return None;
                }
                for span in ["colspan", "rowspan"] {
                    if attr_count(cell, self.txn, span).unwrap_or(1) != 1 {
                        return None;
                    }
                }
                let align = attr_string(cell, self.txn, "align");
                if align
                    .as_deref()
                    .is_some_and(|a| !matches!(a, "left" | "center" | "right"))
                {
                    return None;
                }
                match &alignments[c] {
                    None => alignments[c] = Some(align),
                    Some(seen) if *seen == align => {}
                    Some(_) => return None,
                }
                let mut children = cell.children(self.txn);
                let (Some(XmlOut::Element(paragraph)), None) = (children.next(), children.next())
                else {
                    return None;
                };
                if paragraph.tag().as_ref() != "paragraph" {
                    return None;
                }
                row_paragraphs.push(paragraph);
            }
            paragraphs.push(row_paragraphs);
        }
        Some(PipeTable {
            alignments: alignments.into_iter().map(Option::flatten).collect(),
            rows: paragraphs,
        })
    }

    fn pipe_table(&mut self, table: &PipeTable) -> String {
        let mut lines = Vec::with_capacity(table.rows.len() + 1);
        for (r, row) in table.rows.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .map(|paragraph| {
                    let items = self.element_items(paragraph);
                    // GFM splits the row at `|` before reading anything
                    // inside it, code spans and HTML included.
                    render_inline(&items, Ctx::Cell).replace('|', "\\|")
                })
                .collect();
            lines.push(format!("| {} |", cells.join(" | ")));
            if r == 0 {
                let rule: Vec<&str> = table
                    .alignments
                    .iter()
                    .map(|a| match a.as_deref() {
                        Some("left") => ":---",
                        Some("center") => ":---:",
                        Some("right") => "---:",
                        _ => "---",
                    })
                    .collect();
                lines.push(format!("| {} |", rule.join(" | ")));
            }
        }
        lines.join("\n")
    }

    /// An HTML table block. It holds no blank line, which would end the block.
    fn html_table(&mut self, rows: &[Vec<XmlElementRef>]) -> String {
        let mut out = String::from("<table>\n");
        for row in rows {
            out.push_str("<tr>");
            for cell in row {
                let tag = if cell.tag().as_ref() == "table_header" {
                    "th"
                } else {
                    "td"
                };
                out.push('<');
                out.push_str(tag);
                for span in ["colspan", "rowspan"] {
                    if let Some(n) = attr_count(cell, self.txn, span).filter(|n| *n != 1) {
                        out.push_str(&format!(" {span}=\"{n}\""));
                    }
                }
                if let Some(align) = attr_string(cell, self.txn, "align")
                    .filter(|a| matches!(a.as_str(), "left" | "center" | "right"))
                {
                    out.push_str(&format!(" align=\"{align}\""));
                }
                out.push('>');
                let children: Vec<XmlOut> = cell.children(self.txn).collect();
                out.push_str(&self.html_blocks(&children));
                out.push_str(&format!("</{tag}>"));
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</table>");
        out
    }

    /// Blocks as HTML on one line, in the schema's DOM shapes.
    fn html_blocks(&mut self, children: &[XmlOut]) -> String {
        let mut out = String::new();
        for child in children {
            match child {
                XmlOut::Element(elem) => out.push_str(&self.html_block(elem)),
                XmlOut::Text(text) => {
                    let items = self.text_items(text, &MarkSet::default());
                    out.push_str(&format!("<p>{}</p>", html_inline(&items)));
                }
                XmlOut::Fragment(fragment) => {
                    let inner: Vec<XmlOut> = fragment.children(self.txn).collect();
                    out.push_str(&self.html_blocks(&inner));
                }
            }
        }
        out
    }

    fn html_block(&mut self, elem: &XmlElementRef) -> String {
        let tag = elem.tag().to_string();
        let children: Vec<XmlOut> = elem.children(self.txn).collect();
        match tag.as_str() {
            "heading" => {
                let level = attr_count(elem, self.txn, "level").unwrap_or(1).clamp(1, 6);
                let items = self.element_items(elem);
                format!("<h{level}>{}</h{level}>", html_inline(&items))
            }
            "code_block" => {
                let class = attr_string(elem, self.txn, "language")
                    .filter(|l| !l.is_empty())
                    .map(|l| format!(" class=\"language-{}\"", escape_html(&l)))
                    .unwrap_or_default();
                format!(
                    "<pre><code{class}>{}</code></pre>",
                    escape_html(&self.raw_text(elem))
                )
            }
            "blockquote" => format!("<blockquote>{}</blockquote>", self.html_blocks(&children)),
            "bullet_list" => format!("<ul>{}</ul>", self.html_blocks(&children)),
            "ordered_list" => {
                let start = attr_count(elem, self.txn, "order")
                    .filter(|n| *n != 1)
                    .map(|n| format!(" start=\"{n}\""))
                    .unwrap_or_default();
                format!("<ol{start}>{}</ol>", self.html_blocks(&children))
            }
            "list_item" => format!("<li>{}</li>", self.html_blocks(&children)),
            "horizontal_rule" => "<hr>".to_string(),
            "embedded_document" => embed_element(
                &attr_string(elem, self.txn, "documentUuid").unwrap_or_default(),
                &attr_string(elem, self.txn, "documentTitle")
                    .unwrap_or_else(|| "Untitled".to_string()),
            ),
            _ => {
                let items = self.element_items(elem);
                format!("<p>{}</p>", html_inline(&items))
            }
        }
    }

    // Reading inline content.

    fn element_items(&self, elem: &XmlElementRef) -> Vec<Item> {
        let children: Vec<XmlOut> = elem.children(self.txn).collect();
        self.items_of(&children)
    }

    fn items_of(&self, children: &[XmlOut]) -> Vec<Item> {
        let mut items = Vec::new();
        self.collect_items(children, &MarkSet::default(), &mut items);
        merge_text(items)
    }

    fn collect_items(&self, children: &[XmlOut], outer: &MarkSet, items: &mut Vec<Item>) {
        for child in children {
            match child {
                XmlOut::Text(text) => items.extend(self.text_items(text, outer)),
                XmlOut::Element(elem) => self.collect_element(elem, outer, items),
                XmlOut::Fragment(fragment) => {
                    let inner: Vec<XmlOut> = fragment.children(self.txn).collect();
                    self.collect_items(&inner, outer, items);
                }
            }
        }
    }

    fn collect_element(&self, elem: &XmlElementRef, outer: &MarkSet, items: &mut Vec<Item>) {
        let attr = |name: &str| attr_string(elem, self.txn, name);
        let tag = elem.tag().to_string();
        match tag.as_str() {
            "hard_break" | "br" => items.push(Item::Break),
            "image" => items.push(Item::Image {
                src: self.links.image_src(&attr("src").unwrap_or_default()),
                alt: attr("alt"),
                title: attr("title"),
                width: attr("width"),
                align: attr("align"),
            }),
            "mention" => items.push(Item::Mention {
                uuid: attr("uuid").unwrap_or_default(),
                name: attr("name").unwrap_or_else(|| "unknown".to_string()),
                avatar_url: attr("avatarUrl"),
            }),
            "ticket_link" => {
                let ticket_id = attr("ticketId").unwrap_or_default();
                let number = ticket_id
                    .parse::<i32>()
                    .ok()
                    .and_then(|id| self.numbers.get(&id));
                let label = match number {
                    Some(number) => format!("Ticket #{number}"),
                    None => "Ticket".to_string(),
                };
                items.push(Item::Ticket {
                    ticket_id,
                    href: attr("href").unwrap_or_default(),
                    label,
                });
            }
            // Marks written as elements by older documents.
            _ => {
                let mut marks = outer.clone();
                match tag.as_str() {
                    "strong" | "b" => marks.strong = true,
                    "em" | "i" => marks.em = true,
                    "code" => marks.code = true,
                    "link" | "a" => {
                        marks.link = Some(LinkMark {
                            href: self.links.href(&attr("href").unwrap_or_default()),
                            title: attr("title").filter(|t| !t.is_empty()),
                        })
                    }
                    _ => {}
                }
                let inner: Vec<XmlOut> = elem.children(self.txn).collect();
                self.collect_items(&inner, &marks, items);
            }
        }
    }

    /// A text's runs, through the delta. A text the reader can't walk reads
    /// as no text rather than failing the export.
    fn text_items(&self, text: &XmlTextRef, outer: &MarkSet) -> Vec<Item> {
        let diff = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            text.diff(self.txn, YChange::identity)
        }))
        .unwrap_or_default();
        diff.into_iter()
            .filter_map(|d| {
                let Out::Any(Any::String(s)) = &d.insert else {
                    return None;
                };
                if s.is_empty() {
                    return None;
                }
                let mut marks = marks_from(d.attributes.as_deref());
                marks.strong |= outer.strong;
                marks.em |= outer.em;
                marks.code |= outer.code;
                match &mut marks.link {
                    Some(link) => link.href = self.links.href(&link.href),
                    None => marks.link = outer.link.clone(),
                }
                Some(Item::Text(s.to_string(), marks))
            })
            .collect()
    }

    /// A code block's text: its runs without marks, breaks as line breaks.
    fn raw_text(&self, elem: &XmlElementRef) -> String {
        let mut out = String::new();
        for item in self.element_items(elem) {
            match item {
                Item::Text(text, _) => out.push_str(&text),
                Item::Break => out.push('\n'),
                _ => {}
            }
        }
        out
    }
}

/// A table a GFM pipe table holds exactly.
struct PipeTable {
    alignments: Vec<Option<String>>,
    rows: Vec<Vec<XmlElementRef>>,
}

fn embed_element(uuid: &str, title: &str) -> String {
    format!(
        "<div data-embedded-document=\"true\" data-document-uuid=\"{}\" data-document-title=\"{}\">{}</div>",
        escape_html(uuid),
        escape_html(title),
        escape_html(title)
    )
}

/// Adjacent text runs with the same marks as one run.
fn merge_text(items: Vec<Item>) -> Vec<Item> {
    let mut merged: Vec<Item> = Vec::with_capacity(items.len());
    for item in items {
        if let (Some(Item::Text(prev, prev_marks)), Item::Text(text, marks)) =
            (merged.last_mut(), &item)
        {
            if prev_marks == marks {
                prev.push_str(text);
                continue;
            }
        }
        merged.push(item);
    }
    merged
}

// Writing inline content.

/// Inline content as Markdown, with bold and italic as tags when the
/// delimiter form doesn't parse back to the same marks.
fn render_inline(items: &[Item], ctx: Ctx) -> String {
    let markdown = InlineWriter::render(items, ctx, false);
    if parses_back(&markdown, items) {
        markdown
    } else {
        InlineWriter::render(items, ctx, true)
    }
}

/// A mark that wraps a span of runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wrap {
    Link(LinkMark),
    Strong,
    Em,
}

impl Wrap {
    fn on(&self, marks: &MarkSet) -> bool {
        match self {
            Wrap::Link(link) => marks.link.as_ref() == Some(link),
            Wrap::Strong => marks.strong,
            Wrap::Em => marks.em,
        }
    }

    /// Outermost first when two open at once and last as long.
    fn rank(&self) -> u8 {
        match self {
            Wrap::Link(_) => 0,
            Wrap::Strong => 1,
            Wrap::Em => 2,
        }
    }
}

struct InlineWriter {
    out: String,
    ctx: Ctx,
    /// Bold and italic as `<strong>` and `<em>`.
    tags: bool,
    /// Nothing has been written on the current line yet.
    line_start: bool,
    open: Vec<Wrap>,
}

impl InlineWriter {
    fn new(ctx: Ctx, tags: bool) -> Self {
        InlineWriter {
            out: String::new(),
            ctx,
            tags,
            line_start: ctx != Ctx::Inside,
            open: Vec::new(),
        }
    }

    fn render(items: &[Item], ctx: Ctx, tags: bool) -> String {
        let mut w = InlineWriter::new(ctx, tags);
        for (i, item) in items.iter().enumerate() {
            let line_end = items.get(i + 1).is_none_or(|next| *next == Item::Break);
            match item {
                Item::Text(text, marks) => {
                    w.sync_marks(items, i, marks);
                    if marks.code {
                        w.code_span(text);
                    } else {
                        w.text(text, line_end);
                    }
                }
                Item::Break => {
                    w.close_all();
                    if ctx == Ctx::Block && i + 1 < items.len() {
                        w.out.push_str("\\\n");
                        w.line_start = true;
                    } else {
                        w.push("<br>");
                    }
                }
                Item::Image {
                    src,
                    alt,
                    title,
                    width,
                    align,
                } => {
                    w.close_all();
                    if width.is_some() || align.is_some() {
                        // Markdown has no width; the editor's own element does.
                        w.push(&html_inline(std::slice::from_ref(item)));
                    } else {
                        let mut md = format!(
                            "![{}]({}",
                            escape_inline(alt.as_deref().unwrap_or_default()),
                            destination(src)
                        );
                        if let Some(title) = title {
                            md.push(' ');
                            md.push_str(&link_title(title));
                        }
                        md.push(')');
                        w.push(&md);
                    }
                }
                Item::Mention {
                    uuid,
                    name,
                    avatar_url,
                } => {
                    w.close_all();
                    let mut span = format!(
                        "<span data-mention=\"true\" data-uuid=\"{}\" data-name=\"{}\"",
                        escape_html(uuid),
                        escape_html(name)
                    );
                    if let Some(url) = avatar_url {
                        span.push_str(&format!(" data-avatar-url=\"{}\"", escape_html(url)));
                    }
                    span.push_str(&format!(">@{}</span>", escape_inline(name)));
                    w.push(&span);
                }
                Item::Ticket {
                    ticket_id,
                    href,
                    label,
                } => {
                    w.close_all();
                    w.push(&format!(
                        "<span data-ticket-link=\"true\" data-ticket-id=\"{}\" data-href=\"{}\">{}</span>",
                        escape_html(ticket_id),
                        escape_html(href),
                        escape_inline(label)
                    ));
                }
            }
        }
        w.close_all();
        w.out
    }

    fn push(&mut self, s: &str) {
        self.out.push_str(s);
        self.line_start = false;
    }

    /// Close the wraps the run at `i` doesn't carry (and everything opened
    /// inside them), then open the ones it adds, longest-lasting outermost.
    fn sync_marks(&mut self, items: &[Item], i: usize, marks: &MarkSet) {
        if let Some(keep) = self.open.iter().position(|wrap| !wrap.on(marks)) {
            while self.open.len() > keep {
                self.close_last();
            }
        }
        let mut wanted = Vec::new();
        if let Some(link) = &marks.link {
            wanted.push(Wrap::Link(link.clone()));
        }
        if marks.strong {
            wanted.push(Wrap::Strong);
        }
        if marks.em {
            wanted.push(Wrap::Em);
        }
        wanted.retain(|wrap| !self.open.contains(wrap));
        let extent = |wrap: &Wrap| {
            items[i..]
                .iter()
                .take_while(|item| matches!(item, Item::Text(_, m) if wrap.on(m)))
                .count()
        };
        wanted.sort_by(|a, b| {
            extent(b)
                .cmp(&extent(a))
                .then_with(|| a.rank().cmp(&b.rank()))
        });
        for wrap in wanted {
            let opener = match &wrap {
                Wrap::Link(_) => "[",
                Wrap::Strong if self.tags => "<strong>",
                Wrap::Strong => "**",
                Wrap::Em if self.tags => "<em>",
                Wrap::Em => "*",
            };
            self.push(opener);
            self.open.push(wrap);
        }
    }

    fn close_last(&mut self) {
        let Some(wrap) = self.open.pop() else {
            return;
        };
        let closer = match wrap {
            Wrap::Link(link) => {
                let mut s = format!("]({}", destination(&link.href));
                if let Some(title) = &link.title {
                    s.push(' ');
                    s.push_str(&link_title(title));
                }
                s.push(')');
                s
            }
            Wrap::Strong if self.tags => "</strong>".to_string(),
            Wrap::Strong => "**".to_string(),
            Wrap::Em if self.tags => "</em>".to_string(),
            Wrap::Em => "*".to_string(),
        };
        self.push(&closer);
    }

    fn close_all(&mut self) {
        while !self.open.is_empty() {
            self.close_last();
        }
    }

    /// A code span, its fence longer than any backtick run inside. Nothing
    /// inside one is escaped, so one holding `]` inside link text is a
    /// `<code>` tag instead: an unescaped `]:` at a paragraph's start reads
    /// as a link reference definition.
    fn code_span(&mut self, text: &str) {
        if text.contains(']') && self.open.iter().any(|w| matches!(w, Wrap::Link(_))) {
            self.push(&format!("<code>{}</code>", escape_inline(text)));
            return;
        }
        let fence = "`".repeat(longest_run(text, '`') + 1);
        let pad = text.starts_with('`')
            || text.ends_with('`')
            || (text.starts_with(' ') && text.ends_with(' ') && !text.chars().all(|c| c == ' '));
        let pad = if pad { " " } else { "" };
        let body = text.replace(['\n', '\r'], " ");
        self.push(&format!("{fence}{pad}{body}{pad}{fence}"));
    }

    /// Text with every character that could start syntax escaped.
    fn text(&mut self, text: &str, line_end: bool) {
        let chars: Vec<char> = text.chars().collect();
        let block = self.ctx == Ctx::Block;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let first = self.line_start;
            let last = i + 1 == chars.len();
            let prev = i.checked_sub(1).map(|p| chars[p]);
            let next = chars.get(i + 1).copied();
            self.line_start = false;
            match c {
                '\n' => self.out.push_str("&#10;"),
                '\r' => self.out.push_str("&#13;"),
                ' ' | '\t' if first || (last && line_end && self.ctx != Ctx::Inside) => {
                    self.out.push_str(if c == ' ' { "&#32;" } else { "&#9;" })
                }
                '\\' => self.out.push_str("\\\\"),
                '*' | '`' | '[' | ']' | '<' | '~' | '$' => {
                    self.out.push('\\');
                    self.out.push(c);
                }
                '_' if !(prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric)) =>
                {
                    self.out.push_str("\\_")
                }
                '!' if last => self.out.push_str("\\!"),
                '&' if entity_like(&chars[i..]) => self.out.push_str("\\&"),
                '|' if matches!(self.ctx, Ctx::Block | Ctx::Heading) => self.out.push_str("\\|"),
                '#' if first || self.ctx == Ctx::Heading => self.out.push_str("\\#"),
                '>' | '-' | '+' | '=' | ':' if first && block => {
                    self.out.push('\\');
                    self.out.push(c);
                }
                '0'..='9' if first && block => {
                    // `12.` or `12)` at a line's start is a list marker.
                    let mut end = i;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        self.out.push(chars[end]);
                        end += 1;
                    }
                    if let Some(&p @ ('.' | ')')) = chars.get(end) {
                        self.out.push('\\');
                        self.out.push(p);
                        end += 1;
                    }
                    i = end;
                    continue;
                }
                _ => self.out.push(c),
            }
            i += 1;
        }
    }
}

fn longest_run(text: &str, c: char) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        if ch == c {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

/// Whether the `&` that starts `chars` begins a character reference.
fn entity_like(chars: &[char]) -> bool {
    let rest: String = chars.iter().skip(1).take(34).collect();
    let Some(end) = rest.find(';') else {
        return false;
    };
    let body = &rest[..end];
    if let Some(num) = body.strip_prefix('#') {
        if let Some(hex) = num.strip_prefix(['x', 'X']) {
            return (1..=6).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit());
        }
        return (1..=7).contains(&num.len()) && num.chars().all(|c| c.is_ascii_digit());
    }
    body.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && body.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Text inside a construct that already sits mid-line (link text, alt text,
/// a span's label): escaped, with no line rules.
fn escape_inline(text: &str) -> String {
    let mut w = InlineWriter::new(Ctx::Inside, false);
    w.text(text, false);
    w.out
}

/// A link or image destination: bare when it is simple, in angle brackets
/// when it holds spaces or angle brackets.
fn destination(url: &str) -> String {
    let angle = url.is_empty()
        || url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'));
    let chars: Vec<char> = url.chars().collect();
    let mut out = String::with_capacity(url.len() + 2);
    if angle {
        out.push('<');
    }
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '\\' | '<' | '>' => {
                out.push('\\');
                out.push(c);
            }
            '(' | ')' if !angle => {
                out.push('\\');
                out.push(c);
            }
            '&' if entity_like(&chars[i..]) => out.push_str("\\&"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            _ => out.push(c),
        }
    }
    if angle {
        out.push('>');
    }
    out
}

/// A link or image title, double quoted.
fn link_title(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let mut out = String::from("\"");
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '&' if entity_like(&chars[i..]) => out.push_str("\\&"),
            '\n' => out.push_str("&#10;"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Inline content as HTML, in the schema's DOM shapes.
fn html_inline(items: &[Item]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            Item::Text(text, marks) => {
                let mut open = String::new();
                let mut close = String::new();
                if let Some(link) = &marks.link {
                    open.push_str(&format!("<a href=\"{}\"", escape_html(&link.href)));
                    if let Some(title) = &link.title {
                        open.push_str(&format!(" title=\"{}\"", escape_html(title)));
                    }
                    open.push('>');
                    close.insert_str(0, "</a>");
                }
                for (on, tag) in [
                    (marks.strong, "strong"),
                    (marks.em, "em"),
                    (marks.code, "code"),
                ] {
                    if on {
                        open.push_str(&format!("<{tag}>"));
                        close.insert_str(0, &format!("</{tag}>"));
                    }
                }
                out.push_str(&open);
                out.push_str(&escape_html(text));
                out.push_str(&close);
            }
            Item::Break => out.push_str("<br>"),
            Item::Image {
                src,
                alt,
                title,
                width,
                align,
            } => {
                out.push_str(&format!("<img src=\"{}\"", escape_html(src)));
                for (name, value) in [
                    ("alt", alt),
                    ("title", title),
                    ("width", width),
                    ("data-align", align),
                ] {
                    if let Some(value) = value {
                        out.push_str(&format!(" {name}=\"{}\"", escape_html(value)));
                    }
                }
                out.push('>');
            }
            Item::Mention {
                uuid,
                name,
                avatar_url,
            } => {
                out.push_str(&format!(
                    "<span data-mention=\"true\" data-uuid=\"{}\" data-name=\"{}\"",
                    escape_html(uuid),
                    escape_html(name)
                ));
                if let Some(url) = avatar_url {
                    out.push_str(&format!(" data-avatar-url=\"{}\"", escape_html(url)));
                }
                out.push_str(&format!(">@{}</span>", escape_html(name)));
            }
            Item::Ticket {
                ticket_id,
                href,
                label,
            } => out.push_str(&format!(
                "<span data-ticket-link=\"true\" data-ticket-id=\"{}\" data-href=\"{}\">{}</span>",
                escape_html(ticket_id),
                escape_html(href),
                escape_html(label)
            )),
        }
    }
    out
}

/// A fenced code block, its fence longer than any backtick run inside.
fn code_fence(language: &str, text: &str) -> String {
    let fence = "`".repeat(longest_run(text, '`').max(2) + 1);
    let language: String = language
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '`')
        .collect();
    if text.is_empty() {
        format!("{fence}{language}\n{fence}")
    } else {
        format!("{fence}{language}\n{text}\n{fence}")
    }
}

/// A list item: the marker on the first line, the rest indented to its
/// content column.
fn list_item(marker: &str, body: &str) -> String {
    let pad = " ".repeat(marker.len() + 1);
    let mut out = String::new();
    for (n, line) in body.split('\n').enumerate() {
        if n == 0 {
            out.push_str(marker);
            if !line.is_empty() {
                out.push(' ');
                out.push_str(line);
            }
        } else {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(&pad);
                out.push_str(line);
            }
        }
    }
    out
}

/// Each line behind `>`.
fn quote(body: &str) -> String {
    body.split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `markdown`'s first line opens an HTML block, which runs on until
/// a blank line.
fn opens_html_block(markdown: &str) -> bool {
    matches!(
        Parser::new_ext(markdown, crate::services::docs_markdown::parse::options()).next(),
        Some(Event::Start(Tag::HtmlBlock))
    )
}

// Checking inline Markdown by parsing it back.

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String, MarkSet),
    Atom,
}

fn push_text_token(tokens: &mut Vec<Token>, text: &str, marks: MarkSet) {
    if let Some(Token::Text(prev, prev_marks)) = tokens.last_mut() {
        if *prev_marks == marks {
            prev.push_str(text);
            return;
        }
    }
    tokens.push(Token::Text(text.to_string(), marks));
}

fn expected_tokens(items: &[Item]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for item in items {
        match item {
            Item::Text(text, marks) => push_text_token(&mut tokens, text, marks.clone()),
            _ => tokens.push(Token::Atom),
        }
    }
    tokens
}

/// Whether `markdown`, read as one paragraph by the importer's parser, holds
/// the same text runs with the same marks as `items`, and an inline node
/// wherever they have one.
fn parses_back(markdown: &str, items: &[Item]) -> bool {
    if markdown.is_empty() {
        return items.is_empty();
    }
    let options = crate::services::docs_markdown::parse::options();
    let mut tokens = Vec::new();
    let mut strong = 0u32;
    let mut em = 0u32;
    let mut links: Vec<LinkMark> = Vec::new();
    let mut code = 0u32;
    let mut image_depth = 0u32;
    let mut span_depth = 0u32;
    let mut paragraphs = 0;
    for event in Parser::new_ext(markdown, options) {
        if image_depth > 0 {
            match event {
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(TagEnd::Image) => image_depth -= 1,
                _ => {}
            }
            continue;
        }
        let marks = |code: bool| MarkSet {
            strong: strong > 0,
            em: em > 0,
            code,
            link: links.last().cloned(),
        };
        match event {
            Event::Start(Tag::Paragraph) => {
                paragraphs += 1;
                if paragraphs > 1 {
                    return false;
                }
            }
            Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => em += 1,
            Event::End(TagEnd::Emphasis) => em = em.saturating_sub(1),
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => links.push(LinkMark {
                href: dest_url.to_string(),
                title: (!title.is_empty()).then(|| title.to_string()),
            }),
            Event::End(TagEnd::Link) => {
                links.pop();
            }
            Event::Start(Tag::Image { .. }) => {
                tokens.push(Token::Atom);
                image_depth = 1;
            }
            Event::Text(text) if span_depth == 0 => {
                push_text_token(&mut tokens, &text, marks(code > 0))
            }
            Event::Code(text) if span_depth == 0 => {
                push_text_token(&mut tokens, &text, marks(true))
            }
            Event::Text(_) | Event::Code(_) => {}
            Event::HardBreak => tokens.push(Token::Atom),
            Event::InlineHtml(html) => {
                let tag = html.trim().to_ascii_lowercase();
                if tag.starts_with("<span") {
                    if span_depth == 0 {
                        tokens.push(Token::Atom);
                    }
                    span_depth += 1;
                } else if tag.starts_with("</span") {
                    span_depth = span_depth.saturating_sub(1);
                } else if tag == "<code>" {
                    code += 1;
                } else if tag == "</code>" {
                    code = code.saturating_sub(1);
                } else if tag.starts_with("<br") || tag.starts_with("<img") {
                    tokens.push(Token::Atom);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    tokens == expected_tokens(items)
}

#[cfg(test)]
mod tests;
