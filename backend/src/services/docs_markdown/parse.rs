//! Markdown to the [`ir`](super::ir) tree, through pulldown-cmark.
//!
//! The parser is faithful rather than safe: link and image destinations are
//! kept as written, empty containers are kept, and text may hold `\n` from
//! soft breaks. The normaliser decides what survives. What the parser does
//! decide is how syntax with no editor node maps onto ones that exist
//! (callouts, task lists, footnotes, math, definition lists, raw HTML), and
//! it counts each of those in the notes.
//!
//! Raw HTML is reduced to its text here. Inline formatting tags (`<b>`,
//! `<em>`, `<code>`, `<br>`) map to their marks and breaks; scripts, styles,
//! frames and comments are removed with their content; anything else keeps
//! only its text.

use std::collections::HashMap;

use pulldown_cmark::{
    Alignment as MdAlignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType,
    MetadataBlockKind, Options, Parser, Tag, TagEnd,
};

use super::ir::{
    Alignment, Block, Document, Image, Inline, Link, ListItem, Marks, Table, TableCell, Text,
};
use super::normalise::merge_adjacent_text;
use super::{FrontMatter, FrontMatterKind, NoteCode, Notes};

/// The parser's output, before normalising.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub document: Document,
    pub notes: Notes,
    pub front_matter: Option<FrontMatter>,
}

/// The CommonMark extensions the converter reads.
pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
}

/// Parse one Markdown page.
pub fn parse(markdown: &str) -> Parsed {
    let mut builder = Builder {
        events: Parser::new_ext(markdown, options()),
        notes: Notes::default(),
        footnotes: Footnotes::default(),
        front_matter: None,
        task_list_seen: false,
    };
    let mut blocks = builder.blocks(None);
    blocks.extend(builder.footnotes_section());
    Parsed {
        document: Document { blocks },
        notes: builder.notes,
        front_matter: builder.front_matter,
    }
}

/// The heading that introduces the collected footnotes.
const NOTES_HEADING: &str = "Notes";

struct Builder<'a> {
    events: Parser<'a>,
    notes: Notes,
    footnotes: Footnotes,
    front_matter: Option<FrontMatter>,
    /// Set when a task list marker is read; checked and reset per list.
    task_list_seen: bool,
}

/// Footnotes are numbered by first reference and written out at the end.
#[derive(Default)]
struct Footnotes {
    numbers: HashMap<String, usize>,
    /// Labels in number order.
    order: Vec<String>,
    /// Definitions in source order.
    definitions: Vec<(String, Vec<Block>)>,
}

impl Footnotes {
    fn number(&mut self, label: &str) -> usize {
        if let Some(n) = self.numbers.get(label) {
            return *n;
        }
        self.order.push(label.to_string());
        let n = self.order.len();
        self.numbers.insert(label.to_string(), n);
        n
    }
}

impl Builder<'_> {
    /// Blocks until `end` (or the end of input). Inline content that arrives
    /// without a paragraph, as in tight list items, gets one.
    fn blocks(&mut self, end: Option<TagEnd>) -> Vec<Block> {
        let mut out = Vec::new();
        let mut loose = Inlines::new(true);
        while let Some(event) = self.events.next() {
            match event {
                Event::End(e) if Some(e) == end => break,
                Event::Start(tag) if is_block_tag(&tag) => {
                    loose.flush_into(&mut out);
                    self.block(tag, &mut out);
                }
                Event::Rule => {
                    loose.flush_into(&mut out);
                    out.push(Block::HorizontalRule);
                }
                Event::Html(raw) => {
                    loose.flush_into(&mut out);
                    out.extend(html_block(&raw, &mut self.notes));
                }
                other => self.inline_event(&mut loose, other),
            }
        }
        loose.flush_into(&mut out);
        out
    }

    fn block(&mut self, tag: Tag<'_>, out: &mut Vec<Block>) {
        match tag {
            Tag::Paragraph => {
                let mut inlines = Inlines::new(true);
                self.inlines_until(&mut inlines, TagEnd::Paragraph);
                inlines.flush_into(out);
            }
            Tag::Heading { level, .. } => {
                let mut inlines = Inlines::new(false);
                self.inlines_until(&mut inlines, TagEnd::Heading(level));
                out.push(Block::Heading {
                    level: heading_level(level),
                    content: inlines.into_inlines(),
                });
            }
            Tag::BlockQuote(kind) => {
                let inner = self.blocks(Some(TagEnd::BlockQuote(kind)));
                out.push(self.callout(kind, inner));
            }
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(|word| word.to_lowercase()),
                    CodeBlockKind::Indented => None,
                };
                let mut text = self.text_until(TagEnd::CodeBlock);
                if text.ends_with('\n') {
                    text.pop();
                }
                out.push(Block::CodeBlock { language, text });
            }
            Tag::HtmlBlock => {
                let raw = self.text_until(TagEnd::HtmlBlock);
                out.extend(html_block(&raw, &mut self.notes));
            }
            Tag::List(start) => {
                let outer = std::mem::replace(&mut self.task_list_seen, false);
                let items = self.list_items(TagEnd::List(start.is_some()));
                if self.task_list_seen {
                    self.notes.add(NoteCode::ChecklistsAsBullets);
                }
                self.task_list_seen = outer;
                out.push(match start {
                    Some(n) => Block::OrderedList {
                        order: u32::try_from(n).unwrap_or(u32::MAX),
                        items,
                    },
                    None => Block::BulletList(items),
                });
            }
            Tag::FootnoteDefinition(label) => {
                let blocks = self.blocks(Some(TagEnd::FootnoteDefinition));
                self.footnotes.definitions.push((label.to_string(), blocks));
            }
            Tag::DefinitionList => self.definition_list(out),
            Tag::Table(alignments) => {
                let table = self.table(&alignments);
                out.push(Block::Table(table));
            }
            Tag::MetadataBlock(kind) => {
                let raw = self.text_until(TagEnd::MetadataBlock(kind));
                if self.front_matter.is_none() {
                    self.front_matter = Some(FrontMatter {
                        kind: match kind {
                            MetadataBlockKind::YamlStyle => FrontMatterKind::Yaml,
                            MetadataBlockKind::PlusesStyle => FrontMatterKind::Pluses,
                        },
                        raw,
                    });
                }
            }
            // Containers that only appear inside the ones above. Should one
            // turn up on its own, keep its content.
            other => {
                let end = other.to_end();
                out.extend(self.blocks(Some(end)));
            }
        }
    }

    fn list_items(&mut self, end: TagEnd) -> Vec<ListItem> {
        let mut items = Vec::new();
        while let Some(event) = self.events.next() {
            match event {
                Event::End(e) if e == end => break,
                Event::Start(Tag::Item) => items.push(ListItem {
                    blocks: self.blocks(Some(TagEnd::Item)),
                }),
                _ => {}
            }
        }
        items
    }

    /// A definition list: each term a bold paragraph, each definition its
    /// blocks.
    fn definition_list(&mut self, out: &mut Vec<Block>) {
        while let Some(event) = self.events.next() {
            match event {
                Event::End(TagEnd::DefinitionList) => break,
                Event::Start(Tag::DefinitionListTitle) => {
                    let mut inlines = Inlines::new(false);
                    self.inlines_until(&mut inlines, TagEnd::DefinitionListTitle);
                    let mut term = inlines.into_inlines();
                    for inline in &mut term {
                        if let Inline::Text(t) = inline {
                            t.marks.strong = true;
                        }
                    }
                    out.push(Block::Paragraph(term));
                }
                Event::Start(Tag::DefinitionListDefinition) => {
                    out.extend(self.blocks(Some(TagEnd::DefinitionListDefinition)));
                }
                _ => {}
            }
        }
    }

    /// A GFM table: the head row of header cells, body rows of plain cells,
    /// each cell aligned as its column's delimiter says.
    fn table(&mut self, alignments: &[MdAlignment]) -> Table {
        let alignments: Vec<Option<Alignment>> = alignments
            .iter()
            .map(|a| match a {
                MdAlignment::None => None,
                MdAlignment::Left => Some(Alignment::Left),
                MdAlignment::Center => Some(Alignment::Center),
                MdAlignment::Right => Some(Alignment::Right),
            })
            .collect();
        let mut table = Table::default();
        while let Some(event) = self.events.next() {
            match event {
                Event::End(TagEnd::Table) => break,
                Event::Start(Tag::TableHead) => {
                    let row = self.table_row(TagEnd::TableHead, true, &alignments);
                    table.rows.push(row);
                }
                Event::Start(Tag::TableRow) => {
                    let row = self.table_row(TagEnd::TableRow, false, &alignments);
                    table.rows.push(row);
                }
                _ => {}
            }
        }
        table
    }

    fn table_row(
        &mut self,
        end: TagEnd,
        header: bool,
        alignments: &[Option<Alignment>],
    ) -> Vec<TableCell> {
        let mut cells = Vec::new();
        while let Some(event) = self.events.next() {
            match event {
                Event::End(e) if e == end => break,
                Event::Start(Tag::TableCell) => {
                    let mut inlines = Inlines::new(false);
                    self.inlines_until(&mut inlines, TagEnd::TableCell);
                    let mut cell = TableCell::new(header, inlines.into_inlines());
                    cell.align = alignments.get(cells.len()).copied().flatten();
                    cells.push(cell);
                }
                _ => {}
            }
        }
        cells
    }

    /// A quote, or a callout written as one. A GFM alert (`> [!NOTE]`) or an
    /// Obsidian callout (`> [!tip] Title`) keeps its label as a bold first
    /// paragraph.
    fn callout(&mut self, kind: Option<BlockQuoteKind>, mut inner: Vec<Block>) -> Block {
        let label = match kind {
            Some(kind) => Some(alert_label(kind).to_string()),
            None => take_callout_marker(&mut inner),
        };
        if let Some(label) = label {
            self.notes.add(NoteCode::CalloutsAsQuotes);
            inner.insert(
                0,
                Block::Paragraph(vec![Inline::marked(label, Marks::strong())]),
            );
        }
        Block::Blockquote(inner)
    }

    /// The concatenated text of every event until `end`.
    fn text_until(&mut self, end: TagEnd) -> String {
        let mut text = String::new();
        for event in self.events.by_ref() {
            match event {
                Event::End(e) if e == end => break,
                Event::Text(t) | Event::Html(t) | Event::Code(t) => text.push_str(&t),
                _ => {}
            }
        }
        text
    }

    fn inlines_until(&mut self, inlines: &mut Inlines, end: TagEnd) {
        while let Some(event) = self.events.next() {
            match event {
                Event::End(e) if e == end => break,
                other => self.inline_event(inlines, other),
            }
        }
    }

    fn inline_event(&mut self, st: &mut Inlines, event: Event<'_>) {
        match event {
            Event::Text(t) => st.text(&t),
            Event::Code(t) => st.code(&t),
            Event::InlineMath(t) => {
                self.notes.add(NoteCode::FormulasAsCode);
                st.code(&t);
            }
            Event::DisplayMath(t) => {
                self.notes.add(NoteCode::FormulasAsCode);
                st.display_math(&t);
            }
            Event::InlineHtml(raw) | Event::Html(raw) => self.inline_html(st, &raw),
            Event::FootnoteReference(label) => {
                let n = self.footnotes.number(&label);
                st.text(&format!("[{n}]"));
            }
            Event::SoftBreak => st.text("\n"),
            Event::HardBreak => st.push(Inline::HardBreak),
            Event::TaskListMarker(checked) => {
                self.task_list_seen = true;
                st.push(Inline::text(if checked {
                    "\u{2611} "
                } else {
                    "\u{2610} "
                }));
            }
            Event::Start(Tag::Emphasis) => st.em += 1,
            Event::End(TagEnd::Emphasis) => st.em = st.em.saturating_sub(1),
            Event::Start(Tag::Strong) => st.strong += 1,
            Event::End(TagEnd::Strong) => st.strong = st.strong.saturating_sub(1),
            Event::Start(Tag::Strikethrough | Tag::Superscript | Tag::Subscript) => {
                self.notes.add(NoteCode::StrikethroughRemoved);
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                let href = if link_type == LinkType::Email {
                    format!("mailto:{dest_url}")
                } else {
                    dest_url.to_string()
                };
                st.links.push(Link {
                    href,
                    title: non_empty(&title),
                });
            }
            Event::End(TagEnd::Link) => {
                st.links.pop();
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                if st.image.is_some() {
                    st.nested_images += 1;
                } else {
                    st.image = Some(Image {
                        src: dest_url.to_string(),
                        alt: None,
                        title: non_empty(&title),
                        ..Image::default()
                    });
                }
            }
            Event::End(TagEnd::Image) => {
                if st.nested_images > 0 {
                    st.nested_images -= 1;
                } else if let Some(image) = st.image.take() {
                    st.push(Inline::Image(image));
                }
            }
            _ => {}
        }
    }

    /// One inline HTML tag or comment.
    fn inline_html(&mut self, st: &mut Inlines, raw: &str) {
        let Some((scanned, _)) = scan_tag(raw.trim_start()) else {
            st.text(raw);
            return;
        };
        let tag = match scanned {
            Scanned::Comment | Scanned::Declaration => {
                self.notes.add(NoteCode::HtmlRemoved);
                return;
            }
            Scanned::Tag(tag) => tag,
        };
        if st.suppressed > 0 {
            if is_removed_with_content(&tag.name) {
                if tag.closing {
                    st.suppressed -= 1;
                } else if !tag.self_closing {
                    st.suppressed += 1;
                }
            }
            return;
        }
        let open = !tag.closing;
        let step = |n: &mut u32| {
            if open {
                *n += 1;
            } else {
                *n = n.saturating_sub(1);
            }
        };
        match tag.name.as_str() {
            "br" => st.push(Inline::HardBreak),
            "strong" | "b" => step(&mut st.strong),
            "em" | "i" => step(&mut st.em),
            "code" | "kbd" | "samp" | "tt" => step(&mut st.code),
            name if is_strike_like(name) => {
                if open {
                    self.notes.add(NoteCode::StrikethroughRemoved);
                }
            }
            "img" => self.notes.add(NoteCode::ImagesDropped),
            name if is_removed_with_content(name) => {
                if open {
                    self.notes.add(NoteCode::HtmlRemoved);
                    if !tag.self_closing {
                        st.suppressed += 1;
                    }
                }
            }
            _ => {
                if open {
                    self.notes.add(NoteCode::HtmlAsText);
                }
            }
        }
    }

    /// The collected footnote definitions as a "Notes" heading and an
    /// ordered list, numbered as the text references them.
    fn footnotes_section(&mut self) -> Vec<Block> {
        if self.footnotes.definitions.is_empty() {
            return Vec::new();
        }
        let definitions = std::mem::take(&mut self.footnotes.definitions);
        for (label, _) in &definitions {
            self.footnotes.number(label);
        }
        let mut by_label: HashMap<String, Vec<Block>> = definitions.into_iter().collect();
        let items = self
            .footnotes
            .order
            .iter()
            .map(|label| ListItem {
                blocks: by_label.remove(label).unwrap_or_default(),
            })
            .collect();
        vec![
            Block::Heading {
                level: 2,
                content: vec![Inline::text(NOTES_HEADING)],
            },
            Block::OrderedList { order: 1, items },
        ]
    }
}

/// Inline content being collected, with the marks currently open.
struct Inlines {
    /// Whether display math may split the paragraph around a code block.
    /// False in headings and table cells, which hold inline content only.
    can_split: bool,
    done: Vec<Block>,
    current: Vec<Inline>,
    em: u32,
    strong: u32,
    code: u32,
    links: Vec<Link>,
    /// An image whose alt text is being read.
    image: Option<Image>,
    nested_images: u32,
    /// Depth inside HTML removed with its content (`<script>`, `<style>`).
    suppressed: u32,
}

impl Inlines {
    fn new(can_split: bool) -> Self {
        Inlines {
            can_split,
            done: Vec::new(),
            current: Vec::new(),
            em: 0,
            strong: 0,
            code: 0,
            links: Vec::new(),
            image: None,
            nested_images: 0,
            suppressed: 0,
        }
    }

    fn marks(&self, code: bool) -> Marks {
        Marks {
            link: self.links.last().cloned(),
            em: self.em > 0,
            strong: self.strong > 0,
            code: code || self.code > 0,
        }
    }

    fn push(&mut self, inline: Inline) {
        if self.suppressed > 0 {
            return;
        }
        if self.image.is_some() {
            // Inside an image's alt text only the text counts.
            if let Some(alt) = self.image.as_mut().map(|i| i.alt.get_or_insert_default()) {
                alt.push_str(&super::ir::plain_text(std::slice::from_ref(&inline)));
            }
            return;
        }
        self.current.push(inline);
    }

    fn text(&mut self, text: &str) {
        let marks = self.marks(false);
        self.push(Inline::Text(Text {
            text: text.to_string(),
            marks,
        }));
    }

    fn code(&mut self, text: &str) {
        let marks = self.marks(true);
        self.push(Inline::Text(Text {
            text: text.to_string(),
            marks,
        }));
    }

    /// `$$...$$`: a `latex` code block where blocks are allowed, inline code
    /// elsewhere.
    fn display_math(&mut self, text: &str) {
        if !self.can_split || self.image.is_some() || self.suppressed > 0 {
            self.code(text);
            return;
        }
        if !self.current.is_empty() {
            self.done
                .push(Block::Paragraph(std::mem::take(&mut self.current)));
        }
        self.done.push(Block::CodeBlock {
            language: Some("latex".to_string()),
            text: text.trim_matches('\n').to_string(),
        });
    }

    /// Move what was collected into `out` as blocks.
    fn flush_into(&mut self, out: &mut Vec<Block>) {
        out.append(&mut self.done);
        if !self.current.is_empty() {
            out.push(Block::Paragraph(std::mem::take(&mut self.current)));
        }
    }

    fn into_inlines(self) -> Vec<Inline> {
        self.current
    }
}

fn is_block_tag(tag: &Tag<'_>) -> bool {
    !matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn alert_label(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// An Obsidian-style callout marker, `[!type]` with an optional fold sign
/// and title, at the start of a quote's first line. Removes the marker line
/// and returns the label: the title, or the type capitalised.
fn take_callout_marker(blocks: &mut Vec<Block>) -> Option<String> {
    let Some(Block::Paragraph(inlines)) = blocks.first_mut() else {
        return None;
    };
    merge_adjacent_text(inlines);
    let Some(Inline::Text(first)) = inlines.first_mut() else {
        return None;
    };
    if !first.marks.is_empty() || !first.text.starts_with("[!") {
        return None;
    }
    let line_end = first.text.find('\n').unwrap_or(first.text.len());
    let line = &first.text[..line_end];
    let close = line.find(']')?;
    let kind = &line[2..close];
    if kind.is_empty()
        || !kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let title = line[close + 1..]
        .trim_start_matches(['+', '-'])
        .trim()
        .to_string();
    let label = if title.is_empty() {
        let lower = kind.to_ascii_lowercase();
        let mut chars = lower.chars();
        chars
            .next()
            .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
            .unwrap_or_default()
    } else {
        title
    };

    let rest = first.text[line_end..].trim_start_matches('\n').to_string();
    if rest.is_empty() {
        inlines.remove(0);
    } else {
        first.text = rest;
    }
    let first_is_blank = matches!(inlines.first(), Some(Inline::HardBreak));
    if first_is_blank {
        inlines.remove(0);
    }
    if inlines.is_empty() {
        blocks.remove(0);
    }
    Some(label)
}

// Raw HTML, reduced to text.

/// Tags removed together with everything inside them.
fn is_removed_with_content(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "iframe" | "object" | "embed" | "template" | "noscript" | "svg"
    )
}

/// Formatting the editor has no mark for; the text is kept.
fn is_strike_like(name: &str) -> bool {
    matches!(
        name,
        "del" | "s" | "strike" | "u" | "ins" | "mark" | "sup" | "sub"
    )
}

/// Tags whose boundary separates paragraphs when an HTML block is read as
/// text.
fn is_paragraph_break(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "li"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "pre"
            | "table"
            | "ul"
            | "ol"
            | "dl"
            | "dt"
            | "dd"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "details"
            | "summary"
            | "figure"
            | "figcaption"
            | "hr"
    )
}

#[derive(Debug, PartialEq, Eq)]
struct TagInfo {
    name: String,
    closing: bool,
    self_closing: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Scanned {
    Comment,
    /// `<!DOCTYPE>`, `<?...?>`, `<![CDATA[...]]>`.
    Declaration,
    Tag(TagInfo),
}

/// Read one tag, comment or declaration at the start of `s` (which starts
/// with `<`). Returns it and its length in bytes, or `None` when `s` does
/// not start with one, so the `<` is literal text.
fn scan_tag(s: &str) -> Option<(Scanned, usize)> {
    let rest = s.strip_prefix('<')?;
    if rest.starts_with("!--") {
        let len = s.find("-->").map(|i| i + 3).unwrap_or(s.len());
        return Some((Scanned::Comment, len));
    }
    if rest.starts_with('!') || rest.starts_with('?') {
        let len = s.find('>').map(|i| i + 1).unwrap_or(s.len());
        return Some((Scanned::Declaration, len));
    }
    let (closing, body) = match rest.strip_prefix('/') {
        Some(body) => (true, body),
        None => (false, rest),
    };
    let name_len = body
        .char_indices()
        .take_while(|(i, c)| {
            if *i == 0 {
                c.is_ascii_alphabetic()
            } else {
                c.is_ascii_alphanumeric() || *c == '-'
            }
        })
        .count();
    if name_len == 0 {
        return None;
    }
    let name = body[..name_len].to_ascii_lowercase();
    // Find the closing `>`, skipping quoted attribute values.
    let mut quote: Option<char> = None;
    let mut prev = '\0';
    let offset = s.len() - body.len() + name_len;
    for (i, c) in s[offset..].char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => {
                return Some((
                    Scanned::Tag(TagInfo {
                        name,
                        closing,
                        self_closing: prev == '/',
                    }),
                    offset + i + 1,
                ));
            }
            None => {}
        }
        prev = c;
    }
    None
}

/// An HTML block as paragraphs of its text. Block-level tags separate
/// paragraphs; scripts, styles, frames and comments are removed.
fn html_block(raw: &str, notes: &mut Notes) -> Vec<Block> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    // The tag being removed with its content, and how deeply it is nested.
    let mut removing: Option<(String, u32)> = None;
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with('<') {
            if let Some((scanned, len)) = scan_tag(rest) {
                i += len;
                match scanned {
                    Scanned::Comment | Scanned::Declaration => {
                        if removing.is_none() {
                            notes.add(NoteCode::HtmlRemoved);
                        }
                    }
                    Scanned::Tag(tag) => {
                        if let Some((name, depth)) = removing.as_mut() {
                            if *name == tag.name {
                                if tag.closing {
                                    *depth -= 1;
                                } else if !tag.self_closing {
                                    *depth += 1;
                                }
                            }
                            if *depth == 0 {
                                removing = None;
                            }
                        } else if is_removed_with_content(&tag.name) {
                            if !tag.closing {
                                notes.add(NoteCode::HtmlRemoved);
                                if !tag.self_closing {
                                    removing = Some((tag.name, 1));
                                }
                            }
                        } else if tag.name == "img" {
                            notes.add(NoteCode::ImagesDropped);
                        } else if is_paragraph_break(&tag.name) {
                            paragraphs.push(std::mem::take(&mut current));
                        }
                    }
                }
                continue;
            }
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        if removing.is_none() {
            current.push(c);
        }
        i += c.len_utf8();
    }
    paragraphs.push(current);

    let blocks: Vec<Block> = paragraphs
        .iter()
        .map(|p| {
            html_escape::decode_html_entities(p)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|p| !p.is_empty())
        .map(|p| Block::Paragraph(vec![Inline::text(p)]))
        .collect();
    if !blocks.is_empty() {
        notes.add(NoteCode::HtmlAsText);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_tag_reads_tags_comments_and_literals() {
        assert_eq!(
            scan_tag("<a href=\"x>y\">z"),
            Some((
                Scanned::Tag(TagInfo {
                    name: "a".into(),
                    closing: false,
                    self_closing: false
                }),
                14
            ))
        );
        assert_eq!(
            scan_tag("<br/>"),
            Some((
                Scanned::Tag(TagInfo {
                    name: "br".into(),
                    closing: false,
                    self_closing: true
                }),
                5
            ))
        );
        assert_eq!(scan_tag("<!-- x -->y"), Some((Scanned::Comment, 10)));
        assert_eq!(scan_tag("< 3"), None);
        assert_eq!(scan_tag("<3"), None);
        assert_eq!(scan_tag("<b unterminated"), None);
    }

    #[test]
    fn html_block_keeps_text_and_removes_scripts() {
        let mut notes = Notes::default();
        let blocks = html_block(
            "<div>One &amp; <b>two</b></div><script>alert(1)</script><p>Three</p><!-- c -->",
            &mut notes,
        );
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(vec![Inline::text("One & two")]),
                Block::Paragraph(vec![Inline::text("Three")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 2);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 1);
    }

    #[test]
    fn html_block_of_only_a_comment_is_nothing() {
        let mut notes = Notes::default();
        assert!(html_block("<!-- hidden -->\n", &mut notes).is_empty());
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 1);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    }
}
