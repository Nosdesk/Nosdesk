//! Markdown to the [`ir`](super::ir) tree, through pulldown-cmark.
//!
//! The parser is faithful rather than safe: link and image destinations are
//! kept as written, empty containers are kept, and text may hold `\n` from
//! soft breaks. The normaliser decides what survives. What the parser does
//! decide is how syntax with no editor node maps onto ones that exist
//! (callouts, task lists, footnotes, math, definition lists, raw HTML), and
//! it counts each of those in the notes.
//!
//! HTML blocks are read by [`html`](super::html) into the same tree. Inline
//! HTML is one tag at a time: formatting tags (`<b>`, `<em>`, `<code>`,
//! `<br>`, `<a href>`) map to their marks, breaks and links, `<img>` to an
//! image, the editor's mention and ticket link spans to their nodes when the
//! resolver keeps them; scripts, styles, frames and comments are removed
//! with their content; anything else keeps only its text.
//!
//! The resolver is consulted here only for what changes the tree's shape
//! or needs a node's own label: mentions, ticket links, embedded pages and
//! wikilink embeds. Link and image destinations are the normaliser's.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{
    Alignment as MdAlignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType,
    MetadataBlockKind, OffsetIter, Options, Parser, Tag, TagEnd,
};

use super::html::{self, Container, HtmlTag};
use super::ir::{
    Alignment, Block, Document, Image, Inline, Link, ListItem, Marks, Table, TableCell, Text,
};
use super::normalise::{merge_adjacent_text, Resolver, WikiEmbed, WIKILINK_PREFIX};
use super::{DefaultResolver, FrontMatter, FrontMatterKind, NoteCode, Notes};

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

/// What a source profile switches on for one page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOptions {
    /// Read `[[Note]]` and `![[image.png|300]]` (Obsidian).
    pub wikilinks: bool,
}

/// Parse one Markdown page with no archive around it.
pub fn parse(markdown: &str) -> Parsed {
    parse_with(markdown, ParseOptions::default(), &DefaultResolver)
}

/// Parse one Markdown page, asking `resolver` about mentions, ticket links,
/// embedded pages and wikilink embeds.
pub fn parse_with(markdown: &str, options: ParseOptions, resolver: &dyn Resolver) -> Parsed {
    let mut extensions = self::options();
    if options.wikilinks {
        extensions |= Options::ENABLE_WIKILINKS;
    }
    let mut builder = Builder {
        source: markdown,
        events: Events {
            inner: Parser::new_ext(markdown, extensions).into_offset_iter(),
            range: 0..0,
        },
        resolver,
        notes: Notes::default(),
        footnotes: Footnotes::default(),
        front_matter: None,
        task_list_seen: false,
        open: Vec::new(),
        after_close: None,
    };
    let mut blocks = builder.blocks(None);
    blocks.extend(builder.footnotes_section());
    Parsed {
        document: Document { blocks },
        notes: builder.notes,
        front_matter: builder.front_matter,
    }
}

/// Callouts and toggles an HTML block may leave open around the Markdown
/// after it, one inside another.
const MAX_OPEN_CONTAINERS: usize = 8;

/// The heading that introduces the collected footnotes.
const NOTES_HEADING: &str = "Notes";

/// The parser's events, remembering the source range of the last one.
struct Events<'a> {
    inner: OffsetIter<'a>,
    range: Range<usize>,
}

impl<'a> Iterator for Events<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        let (event, range) = self.inner.next()?;
        self.range = range;
        Some(event)
    }
}

struct Builder<'a> {
    source: &'a str,
    events: Events<'a>,
    resolver: &'a dyn Resolver,
    notes: Notes,
    footnotes: Footnotes,
    front_matter: Option<FrontMatter>,
    /// Set when a task list marker is read; checked and reset per list.
    task_list_seen: bool,
    /// Containers an HTML block at the top level opened and left open.
    open: Vec<Container>,
    /// What followed the closing tag of an open container, still to read.
    after_close: Option<String>,
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
    ///
    /// At the top level (`end` is `None`) an HTML block may open a callout or
    /// toggle that the blocks after it fill, until an HTML block closes it.
    fn blocks(&mut self, end: Option<TagEnd>) -> Vec<Block> {
        let mut out = Vec::new();
        let mut loose = Inlines::new(true);
        while let Some(event) = self.events.next() {
            match event {
                Event::End(e) if Some(e) == end => break,
                Event::Start(Tag::HtmlBlock) => {
                    loose.flush_into(&mut out);
                    let raw = self.text_until(TagEnd::HtmlBlock);
                    if end.is_none() {
                        if let Some(&container) = self.open.last() {
                            if let Some(rest) = html::after_close(&raw, container) {
                                self.open.pop();
                                self.after_close = Some(rest.to_string());
                                break;
                            }
                        }
                        self.top_html(&raw, &mut out);
                    } else {
                        self.html_blocks(&raw, &mut out);
                    }
                }
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
                    self.html_blocks(&raw, &mut out);
                }
                other => self.inline_event(&mut loose, other),
            }
        }
        loose.flush_into(&mut out);
        out
    }

    /// An HTML block's content.
    fn html_blocks(&mut self, raw: &str, out: &mut Vec<Block>) {
        out.extend(html::read_block(raw, self.resolver, &mut self.notes).blocks);
    }

    /// An HTML block at the top level. When it leaves an `<aside>` or
    /// `<details>` open, the blocks up to its closing tag go inside.
    fn top_html(&mut self, raw: &str, out: &mut Vec<Block>) {
        let read = html::read_block(raw, self.resolver, &mut self.notes);
        out.extend(read.blocks);
        // Each open container is a level of recursion: bound it.
        let left_open = read
            .left_open
            .filter(|_| self.open.len() < MAX_OPEN_CONTAINERS);
        if let Some(container) = left_open {
            self.open.push(container);
            let more = self.blocks(None);
            // Input that ends first leaves the container open: close it.
            if self.open.last() == Some(&container) {
                self.open.pop();
            }
            match (container, out.last_mut()) {
                (Container::Aside, Some(Block::Blockquote(inner))) => inner.extend(more),
                _ => out.extend(more),
            }
            if let Some(rest) = self.after_close.take() {
                self.top_html(&rest, out);
            }
        }
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
                let start = self.events.range.start;
                let inner = self.blocks(Some(TagEnd::BlockQuote(kind)));
                let escaped = marker_is_escaped(&self.source[start..]);
                out.push(self.callout(kind, inner, escaped));
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
                self.html_blocks(&raw, out);
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
                    // The parser drops cells past the header's count without
                    // a trace; count them from the source.
                    let source = &self.source[self.events.range.clone()];
                    if source_cells(source) > alignments.len() {
                        self.notes.add(NoteCode::TableCellsDropped);
                    }
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
                    inlines.cell = true;
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
    /// paragraph. A marker written escaped (`> \[!NOTE]`) is text.
    fn callout(
        &mut self,
        kind: Option<BlockQuoteKind>,
        mut inner: Vec<Block>,
        escaped: bool,
    ) -> Block {
        let label = match kind {
            Some(kind) => Some(alert_label(kind).to_string()),
            None if escaped => None,
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
                let href = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    LinkType::WikiLink { .. } => format!("{WIKILINK_PREFIX}{dest_url}"),
                    _ => dest_url.to_string(),
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
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }) => self.wiki_embed(st, &dest_url, has_pothole),
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
                if let Some(embed) = st.wiki.pop() {
                    self.end_wiki_embed(st, embed);
                } else if st.nested_images > 0 {
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
        // GFM unescapes `\|` in a table cell before reading anything in it,
        // HTML included; the parser leaves it in raw HTML.
        let unescaped;
        let raw = if st.cell && raw.contains("\\|") {
            unescaped = raw.replace("\\|", "|");
            unescaped.as_str()
        } else {
            raw
        };
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
            if html::is_removed_with_content(&tag.name) {
                if tag.closing {
                    st.suppressed -= 1;
                } else if !tag.self_closing {
                    st.suppressed += 1;
                }
            }
            return;
        }
        if st.label_depth > 0 {
            // Inside a mention or ticket link span, whose label is replaced.
            if tag.name == "span" {
                if tag.closing {
                    st.label_depth -= 1;
                } else if !tag.self_closing {
                    st.label_depth += 1;
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
            "a" if open => {
                let attrs = html::read_tag(raw);
                match attrs.as_ref().and_then(|t| t.attr("href")) {
                    Some(href) => {
                        st.links.push(Link {
                            href: href.to_string(),
                            title: attrs
                                .as_ref()
                                .and_then(|t| t.attr("title"))
                                .and_then(non_empty),
                        });
                        st.html_links += 1;
                    }
                    None => st.html_anchors += 1,
                }
            }
            "a" => {
                // Close what an `<a>` opened, never a Markdown link.
                if st.html_anchors > 0 {
                    st.html_anchors -= 1;
                } else if st.html_links > 0 {
                    st.html_links -= 1;
                    st.links.pop();
                }
            }
            "img" => {
                if let Some(tag) = html::read_tag(raw) {
                    st.push(Inline::Image(Image {
                        src: tag.attr("src").unwrap_or_default().to_string(),
                        alt: tag.attr("alt").and_then(non_empty),
                        title: tag.attr("title").and_then(non_empty),
                        width: tag.attr("width").and_then(html::parse_width),
                        align: tag.attr("data-align").and_then(image_align),
                    }));
                }
            }
            "span" if open => match html::read_tag(raw) {
                Some(span) if span.attr("data-mention").is_some() => self.mention(st, &span),
                Some(span) if span.attr("data-ticket-link").is_some() => self.ticket(st, &span),
                _ => self.notes.add(NoteCode::HtmlAsText),
            },
            name if html::is_strike_like(name) => {
                if open {
                    self.notes.add(NoteCode::StrikethroughRemoved);
                }
            }
            name if html::is_removed_with_content(name) => {
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

    /// `<span data-mention>`: the mention, or "@name". Its label is skipped
    /// either way.
    fn mention(&mut self, st: &mut Inlines, span: &HtmlTag) {
        let uuid = span.attr("data-uuid").unwrap_or_default().to_string();
        let name = span.attr("data-name").unwrap_or_default().to_string();
        if !uuid.trim().is_empty() && !name.trim().is_empty() && self.resolver.mention(&uuid) {
            st.push(Inline::Mention {
                uuid,
                name,
                avatar_url: span
                    .attr("data-avatar-url")
                    .filter(|u| u.starts_with('/') && !u.starts_with("//"))
                    .map(str::to_string),
            });
            st.label_depth = 1;
        } else {
            self.notes.add(NoteCode::MentionsAsText);
            if name.trim().is_empty() {
                // No name to write: the label stands in for it.
                return;
            }
            st.push_plain(&format!("@{name}"));
            st.label_depth = 1;
        }
    }

    /// `<span data-ticket-link>`: the ticket link, or its label as text.
    fn ticket(&mut self, st: &mut Inlines, span: &HtmlTag) {
        let ticket_id = span.attr("data-ticket-id").unwrap_or_default().to_string();
        let href = span.attr("data-href").unwrap_or_default();
        match self.resolver.ticket(&ticket_id, href) {
            Some(href) if !ticket_id.trim().is_empty() => {
                st.push(Inline::TicketLink { ticket_id, href });
                st.label_depth = 1;
            }
            _ => {
                // The label ("Ticket #7") is read as the text it is.
                self.notes.add(NoteCode::TicketLinksAsText);
            }
        }
    }

    /// `![[target]]` or `![[target|300]]`: an embedded page, an image, or
    /// the target's text.
    fn wiki_embed(&mut self, st: &mut Inlines, target: &str, has_pothole: bool) {
        let embed = match self.resolver.wiki_embed(target) {
            WikiEmbed::Page {
                document_uuid,
                document_title,
            } => Wiki::Page(Block::EmbeddedDocument {
                document_uuid,
                document_title,
            }),
            WikiEmbed::Image => Wiki::Image(Image {
                src: format!("{WIKILINK_PREFIX}{target}"),
                ..Image::default()
            }),
            WikiEmbed::Missing => Wiki::Missing(target.to_string()),
        };
        st.wiki.push(WikiState {
            embed,
            has_pothole,
            text: String::new(),
        });
    }

    fn end_wiki_embed(&mut self, st: &mut Inlines, state: WikiState) {
        let label = state.has_pothole.then(|| state.text.trim().to_string());
        match state.embed {
            Wiki::Page(block) => st.block(block),
            Wiki::Image(mut image) => {
                if let Some(label) = label.filter(|l| !l.is_empty()) {
                    match wiki_image_width(&label) {
                        Some(width) => image.width = Some(width),
                        None => image.alt = Some(label),
                    }
                }
                st.push(Inline::Image(image));
            }
            Wiki::Missing(target) => {
                let text = label.filter(|l| !l.is_empty()).unwrap_or(target);
                st.text(&text);
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
    /// Depth inside a mention or ticket link span whose label is replaced
    /// by its node.
    label_depth: u32,
    /// `<a>` tags with no `href` open; their `</a>` closes no link.
    html_anchors: u32,
    /// Links opened by `<a href>`, which only `</a>` closes.
    html_links: u32,
    /// Wikilink embeds whose label is being read.
    wiki: Vec<WikiState>,
    /// In a GFM table cell.
    cell: bool,
}

/// A wikilink embed being read.
struct WikiState {
    embed: Wiki,
    has_pothole: bool,
    /// The label after `|`.
    text: String,
}

enum Wiki {
    Page(Block),
    Image(Image),
    Missing(String),
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
            label_depth: 0,
            html_anchors: 0,
            html_links: 0,
            wiki: Vec::new(),
            cell: false,
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
        if self.suppressed > 0 || self.label_depth > 0 {
            return;
        }
        if let Some(wiki) = self.wiki.last_mut() {
            // Inside a wikilink embed only its label text counts.
            wiki.text
                .push_str(&super::ir::plain_text(std::slice::from_ref(&inline)));
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

    /// Text standing in for a node: only a link around it carries over.
    fn push_plain(&mut self, text: &str) {
        let marks = Marks {
            link: self.links.last().cloned(),
            ..Marks::default()
        };
        self.push(Inline::Text(Text {
            text: text.to_string(),
            marks,
        }));
    }

    /// A block found in inline content (an embedded page): the paragraph
    /// splits around it where blocks are allowed. Elsewhere it is its title.
    fn block(&mut self, block: Block) {
        if self.suppressed > 0 || self.label_depth > 0 {
            return;
        }
        if !self.can_split || self.image.is_some() {
            if let Block::EmbeddedDocument { document_title, .. } = &block {
                let title = document_title.clone();
                self.text(&title);
            }
            return;
        }
        if !self.current.is_empty() {
            self.done
                .push(Block::Paragraph(std::mem::take(&mut self.current)));
        }
        self.done.push(block);
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

// Raw HTML.

fn image_align(value: &str) -> Option<super::ir::ImageAlign> {
    use super::ir::ImageAlign;
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(ImageAlign::Left),
        "center" => Some(ImageAlign::Center),
        "right" => Some(ImageAlign::Right),
        _ => None,
    }
}

/// An Obsidian image size, `300` or `300x200`: the width.
fn wiki_image_width(label: &str) -> Option<u32> {
    let width = label.split_once('x').map(|(w, _)| w).unwrap_or(label);
    if width.is_empty() || !width.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some((_, h)) = label.split_once('x') {
        if h.is_empty() || !h.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    html::parse_width(width)
}

/// Whether the quote at the start of `source` opens with an escaped `\[`,
/// so a `[!type]` marker after it is text.
fn marker_is_escaped(source: &str) -> bool {
    let rest = source.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('>').unwrap_or(rest);
    let rest = rest.strip_prefix([' ', '\t']).unwrap_or(rest);
    rest.starts_with("\\[")
}

/// The cells a GFM table row's source holds: split at each `|` that isn't
/// escaped (code spans included, as GFM does), leading and trailing pipes
/// not counted.
fn source_cells(row: &str) -> usize {
    let row = row.trim();
    let row = row.strip_prefix('|').unwrap_or(row);
    let mut pipes = Vec::new();
    let mut escaped = false;
    for (i, b) in row.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if b == b'\\' {
            escaped = true;
        } else if b == b'|' {
            pipes.push(i);
        }
    }
    let trailing = pipes.last().is_some_and(|&i| i + 1 == row.len());
    if trailing {
        pipes.len()
    } else {
        pipes.len() + 1
    }
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
    fn source_cells_count_unescaped_pipes() {
        assert_eq!(source_cells("| a | b |\n"), 2);
        assert_eq!(source_cells("a | b | c"), 3);
        assert_eq!(source_cells("| a \\| b |"), 1);
        assert_eq!(source_cells("| `x|y` | z |"), 3);
        assert_eq!(source_cells("|"), 1);
    }

    #[test]
    fn an_escaped_marker_is_seen_in_the_source() {
        assert!(marker_is_escaped("> \\[!NOTE]\n> x"));
        assert!(!marker_is_escaped("> [!NOTE]"));
        assert!(!marker_is_escaped(">\n> \\[!NOTE]"));
    }

    #[test]
    fn obsidian_sizes_are_widths() {
        assert_eq!(wiki_image_width("300"), Some(300));
        assert_eq!(wiki_image_width("300x200"), Some(300));
        assert_eq!(wiki_image_width("a diagram"), None);
        assert_eq!(wiki_image_width("x200"), None);
    }
}
