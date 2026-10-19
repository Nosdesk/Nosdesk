//! Makes an [`ir`](super::ir) tree valid under the editor schema.
//!
//! y-prosemirror deletes any node it cannot build, so every rule here
//! prevents a deletion:
//!
//! - `doc` holds at least one block (an empty paragraph when nothing else).
//! - A `list_item` starts with a paragraph (an empty one is inserted before a
//!   leading code block, quote or list); a list holds at least one item.
//! - A `blockquote` holds at least one block; an empty quote is dropped.
//! - A `code_block` holds plain text with no marks (the IR type guarantees
//!   it; math is flattened into it as text).
//! - Headings hold inline content only (the IR type guarantees it); `level`
//!   is 1 to 6.
//! - Nesting deeper than [`MAX_DEPTH`] quotes and lists flattens into the
//!   deepest allowed level.
//! - Text runs are never empty, never hold line breaks, and adjacent runs
//!   with the same marks are merged; code text carries no `strong` or `em`.
//! - Links and images keep only destinations the [`Resolver`] accepts; the
//!   rest keep their text (or alt text).
//! - An `image` has a `src`; `mention`, `ticket_link` and
//!   `embedded_document` have their required attrs.
//! - Tables live at the top level only: one inside a list or quote moves to
//!   after its top-level block, and one inside a table cell becomes a
//!   paragraph per row ("cell · cell").
//! - A table has at least one row and its rows are the same width: short
//!   rows are padded with empty cells, spans running past the table are
//!   trimmed, and a table with no cells is dropped.
//! - A cell holds `cell_block`s only (paragraphs, lists, code blocks), at
//!   least one: headings become bold paragraphs, quotes give up their
//!   content, embedded pages become their title, rules are dropped, and an
//!   empty cell holds an empty paragraph.

use super::ir::{
    blocks_text, Block, Document, Image, Inline, Link, ListItem, Table, TableCell, Text,
};
use super::{NoteCode, Notes};

/// How many quotes and lists may nest. Deeper content is lifted into the
/// innermost allowed level.
pub const MAX_DEPTH: usize = 16;

/// Longest code block language kept; longer info strings are not languages.
const MAX_LANGUAGE_LEN: usize = 40;

/// Decides where links and images may point. A profile or an archive's link
/// map implements this to rewrite relative paths; returning `None` keeps the
/// text and drops the destination.
pub trait Resolver {
    /// The `href` a link should carry, or `None` to keep only its text.
    fn link(&self, href: &str) -> Option<String>;
    /// The `src` an image should carry, or `None` to keep only its alt text.
    fn image(&self, src: &str) -> Option<String>;
}

/// The policy with no archive around the page: web, mail and phone links are
/// kept; web images are kept for a later step to fetch. Relative paths,
/// fragments and every other scheme (`javascript:`, `file:`, `data:`) are
/// dropped.
pub struct DefaultResolver;

impl Resolver for DefaultResolver {
    fn link(&self, href: &str) -> Option<String> {
        let href = href.trim();
        match scheme(href)?.as_str() {
            "http" | "https" => has_host(href).then(|| href.to_string()),
            "mailto" | "tel" => (href.len() > href.find(':')? + 1).then(|| href.to_string()),
            _ => None,
        }
    }

    fn image(&self, src: &str) -> Option<String> {
        let src = src.trim();
        match scheme(src)?.as_str() {
            "http" | "https" => has_host(src).then(|| src.to_string()),
            _ => None,
        }
    }
}

/// The lowercased URL scheme, when `url` has a well-formed one.
fn scheme(url: &str) -> Option<String> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

fn has_host(url: &str) -> bool {
    url.find("://")
        .map(|i| &url[i + 3..])
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| !matches!(c, '/' | '?' | '#') && !c.is_whitespace())
}

/// Normalise a parsed document. Every degrade is counted in `notes`.
pub fn normalise(document: Document, resolver: &dyn Resolver, notes: &mut Notes) -> Document {
    let mut normaliser = Normaliser {
        resolver,
        notes,
        flattening: false,
        in_cell: false,
        hoisted: Vec::new(),
    };
    let mut blocks = Vec::with_capacity(document.blocks.len());
    for block in document.blocks {
        normaliser.block(block, 0, &mut blocks);
        blocks.append(&mut normaliser.hoisted);
    }
    if blocks.is_empty() {
        blocks.push(Block::Paragraph(Vec::new()));
    }
    Document { blocks }
}

struct Normaliser<'a> {
    resolver: &'a dyn Resolver,
    notes: &'a mut Notes,
    /// Inside content being flattened, so one too-deep branch counts once.
    flattening: bool,
    /// Inside a table cell, where a table can only be text.
    in_cell: bool,
    /// Tables found inside a list or quote, to follow their top-level block.
    hoisted: Vec<Block>,
}

impl Normaliser<'_> {
    /// `depth` is the number of quotes and lists around these blocks.
    fn blocks(&mut self, blocks: Vec<Block>, depth: usize) -> Vec<Block> {
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            self.block(block, depth, &mut out);
        }
        out
    }

    fn block(&mut self, block: Block, depth: usize, out: &mut Vec<Block>) {
        match block {
            Block::Paragraph(inlines) => {
                let inlines = self.inlines(inlines);
                if !inlines.is_empty() {
                    out.push(Block::Paragraph(inlines));
                }
            }
            Block::Heading { level, content } => out.push(Block::Heading {
                level: level.clamp(1, 6),
                content: self.inlines(content),
            }),
            Block::Blockquote(inner) => {
                if depth >= MAX_DEPTH {
                    self.flatten(inner, depth, out);
                    return;
                }
                let inner = self.blocks(inner, depth + 1);
                if !inner.is_empty() {
                    out.push(Block::Blockquote(inner));
                }
            }
            Block::CodeBlock { language, text } => out.push(Block::CodeBlock {
                language: language.and_then(|l| clean_language(&l)),
                text: text.replace("\r\n", "\n").replace('\r', "\n"),
            }),
            Block::HorizontalRule => out.push(Block::HorizontalRule),
            Block::BulletList(items) => {
                if let Some(items) = self.list_items(items, depth, out) {
                    out.push(Block::BulletList(items));
                }
            }
            Block::OrderedList { order, items } => {
                if let Some(items) = self.list_items(items, depth, out) {
                    out.push(Block::OrderedList { order, items });
                }
            }
            Block::EmbeddedDocument {
                document_uuid,
                document_title,
            } => {
                let title = document_title.trim();
                let title = if title.is_empty() { "Untitled" } else { title };
                if document_uuid.trim().is_empty() {
                    self.notes.add(NoteCode::LinksDropped);
                    out.push(Block::Paragraph(vec![Inline::text(title)]));
                } else {
                    out.push(Block::EmbeddedDocument {
                        document_uuid,
                        document_title: title.to_string(),
                    });
                }
            }
            Block::Table(table) => {
                if self.in_cell {
                    self.notes.add(NoteCode::TableCellsFlattened);
                    out.extend(text_paragraphs(&[Block::Table(table)]));
                } else if let Some(table) = self.table(table) {
                    if depth == 0 {
                        out.push(Block::Table(table));
                    } else {
                        self.notes.add(NoteCode::TablesHoisted);
                        self.hoisted.push(Block::Table(table));
                    }
                }
            }
        }
    }

    /// A table with valid cells and rows of one width; `None` when it has no
    /// cells at all.
    fn table(&mut self, table: Table) -> Option<Table> {
        let outer = std::mem::replace(&mut self.in_cell, true);
        let rows: Vec<Vec<TableCell>> = table
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(|cell| self.cell(cell)).collect())
            .collect();
        self.in_cell = outer;
        let (rows, changed) = even_rows(rows)?;
        if changed {
            self.notes.add(NoteCode::TableRowsPadded);
        }
        Some(Table { rows })
    }

    fn cell(&mut self, cell: TableCell) -> TableCell {
        let colspan = cell.colspan.max(1);
        let blocks = self.blocks(cell.blocks, 0);
        let mut content = Vec::with_capacity(blocks.len());
        self.cell_blocks(blocks, &mut content);
        if content.is_empty() {
            content.push(Block::Paragraph(Vec::new()));
        }
        TableCell {
            header: cell.header,
            colspan,
            rowspan: cell.rowspan.max(1),
            colwidth: cell
                .colwidth
                .filter(|w| w.len() == colspan as usize && w.iter().all(|w| *w > 0)),
            align: cell.align,
            blocks: content,
        }
    }

    /// Normalised blocks as a cell's direct children: `cell_block`s only.
    fn cell_blocks(&mut self, blocks: Vec<Block>, out: &mut Vec<Block>) {
        for block in blocks {
            match block {
                Block::Paragraph(_)
                | Block::BulletList(_)
                | Block::OrderedList { .. }
                | Block::CodeBlock { .. } => out.push(block),
                Block::Heading { content, .. } => {
                    self.notes.add(NoteCode::TableCellsFlattened);
                    let mut content = content;
                    for inline in &mut content {
                        if let Inline::Text(text) = inline {
                            text.marks.strong = !text.marks.code;
                        }
                    }
                    merge_adjacent_text(&mut content);
                    if !content.is_empty() {
                        out.push(Block::Paragraph(content));
                    }
                }
                Block::Blockquote(inner) => {
                    self.notes.add(NoteCode::TableCellsFlattened);
                    self.cell_blocks(inner, out);
                }
                Block::HorizontalRule => self.notes.add(NoteCode::TableCellsFlattened),
                Block::EmbeddedDocument { document_title, .. } => {
                    self.notes.add(NoteCode::TableCellsFlattened);
                    out.push(Block::Paragraph(vec![Inline::text(document_title)]));
                }
                // `block` already turned tables in a cell into paragraphs.
                Block::Table(table) => out.extend(text_paragraphs(&[Block::Table(table)])),
            }
        }
    }

    /// A list's items, each starting with a paragraph. `None` when the list
    /// ends up with no items, or when it is too deep and its content went
    /// into `out` instead.
    fn list_items(
        &mut self,
        items: Vec<ListItem>,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Option<Vec<ListItem>> {
        if depth >= MAX_DEPTH {
            let blocks = items.into_iter().flat_map(|item| item.blocks).collect();
            self.flatten(blocks, depth, out);
            return None;
        }
        let items: Vec<ListItem> = items
            .into_iter()
            .map(|item| {
                let mut blocks = self.blocks(item.blocks, depth + 1);
                if !matches!(blocks.first(), Some(Block::Paragraph(_))) {
                    blocks.insert(0, Block::Paragraph(Vec::new()));
                }
                ListItem { blocks }
            })
            .collect();
        (!items.is_empty()).then_some(items)
    }

    /// A too-deep container's blocks, lifted into `out` at this depth.
    fn flatten(&mut self, blocks: Vec<Block>, depth: usize, out: &mut Vec<Block>) {
        let outer = std::mem::replace(&mut self.flattening, true);
        if !outer {
            self.notes.add(NoteCode::NestingFlattened);
        }
        for block in blocks {
            self.block(block, depth, out);
        }
        self.flattening = outer;
    }

    fn inlines(&mut self, inlines: Vec<Inline>) -> Vec<Inline> {
        let mut out = Vec::with_capacity(inlines.len());
        // The last dropped link, so a link split across runs counts once.
        let mut last_dropped: Option<Link> = None;
        for inline in inlines {
            match inline {
                Inline::Text(mut text) => {
                    if text.marks.code {
                        text.marks.em = false;
                        text.marks.strong = false;
                    }
                    if let Some(link) = text.marks.link.take() {
                        text.marks.link = self.link(&link);
                        if text.marks.link.is_none() {
                            if last_dropped.as_ref() != Some(&link) {
                                self.notes.add(NoteCode::LinksDropped);
                            }
                            last_dropped = Some(link);
                        } else {
                            last_dropped = None;
                        }
                    } else {
                        last_dropped = None;
                    }
                    let text = Text {
                        text: single_line(&text.text),
                        marks: text.marks,
                    };
                    if !text.text.is_empty() {
                        out.push(Inline::Text(text));
                    }
                }
                Inline::Image(image) => {
                    last_dropped = None;
                    self.image(image, &mut out);
                }
                Inline::HardBreak => {
                    last_dropped = None;
                    out.push(Inline::HardBreak);
                }
                Inline::TicketLink { ticket_id, href } => {
                    last_dropped = None;
                    if ticket_id.trim().is_empty() || href.trim().is_empty() {
                        self.notes.add(NoteCode::LinksDropped);
                        let label = if href.trim().is_empty() {
                            ticket_id
                        } else {
                            href
                        };
                        if !label.trim().is_empty() {
                            out.push(Inline::text(label));
                        }
                    } else {
                        out.push(Inline::TicketLink { ticket_id, href });
                    }
                }
                Inline::Mention {
                    uuid,
                    name,
                    avatar_url,
                } => {
                    last_dropped = None;
                    if uuid.trim().is_empty() || name.trim().is_empty() {
                        if !name.trim().is_empty() {
                            out.push(Inline::text(format!("@{name}")));
                        }
                    } else {
                        out.push(Inline::Mention {
                            uuid,
                            name,
                            avatar_url: avatar_url.filter(|u| !u.is_empty()),
                        });
                    }
                }
            }
        }
        merge_adjacent_text(&mut out);
        out
    }

    fn link(&self, link: &Link) -> Option<Link> {
        if link.href.trim().is_empty() {
            return None;
        }
        self.resolver.link(&link.href).map(|href| Link {
            href,
            title: link.title.clone().filter(|t| !t.is_empty()),
        })
    }

    fn image(&mut self, image: Image, out: &mut Vec<Inline>) {
        let alt = image.alt.map(|a| single_line(&a)).filter(|a| !a.is_empty());
        let src = if image.src.trim().is_empty() {
            None
        } else {
            self.resolver.image(&image.src)
        };
        match src {
            Some(src) => out.push(Inline::Image(Image {
                src,
                alt,
                title: image
                    .title
                    .map(|t| single_line(&t))
                    .filter(|t| !t.is_empty()),
                width: image.width.filter(|w| *w > 0),
                align: image.align,
            })),
            None => {
                self.notes.add(NoteCode::ImagesDropped);
                if let Some(alt) = alt {
                    out.push(Inline::text(alt));
                }
            }
        }
    }
}

/// Text on one line: soft breaks and stray line endings become spaces.
fn single_line(text: &str) -> String {
    if !text.contains(['\n', '\r']) {
        return text.to_string();
    }
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// A code block language: the info string's first word, lowercased, when it
/// looks like a language name.
fn clean_language(language: &str) -> Option<String> {
    let language = language.trim().to_ascii_lowercase();
    let valid = !language.is_empty()
        && language.len() <= MAX_LANGUAGE_LEN
        && language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-'));
    valid.then_some(language)
}

/// Merge adjacent text runs with the same marks.
pub(crate) fn merge_adjacent_text(inlines: &mut Vec<Inline>) {
    let mut merged: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines.drain(..) {
        if let (Some(Inline::Text(prev)), Inline::Text(next)) = (merged.last_mut(), &inline) {
            if prev.marks == next.marks {
                prev.text.push_str(&next.text);
                continue;
            }
        }
        merged.push(inline);
    }
    *inlines = merged;
}

/// Blocks as paragraphs of their plain text, one per line.
fn text_paragraphs(blocks: &[Block]) -> Vec<Block> {
    blocks_text(blocks)
        .into_iter()
        .map(|line| Block::Paragraph(vec![Inline::text(single_line(&line))]))
        .collect()
}

/// Rows of one width, laid out the way prosemirror-tables' `TableMap` reads
/// them: each cell takes the next free column, and a `rowspan` holds its
/// columns in the rows below. Spans that run past the last row or into
/// another cell are trimmed, and each short row gets empty cells to fill its
/// free columns. Returns the rows and whether anything changed, or `None`
/// when no row has a cell.
fn even_rows(mut rows: Vec<Vec<TableCell>>) -> Option<(Vec<Vec<TableCell>>, bool)> {
    let height = rows.len();
    let mut taken: Vec<Vec<bool>> = vec![Vec::new(); height];
    let is_taken = |taken: &[Vec<bool>], r: usize, c: usize| taken[r].get(c).copied() == Some(true);
    let mut changed = false;
    for (r, row) in rows.iter_mut().enumerate() {
        let mut col = 0;
        for cell in row {
            while is_taken(&taken, r, col) {
                col += 1;
            }
            let rowspan = cell
                .rowspan
                .min(u32::try_from(height - r).unwrap_or(u32::MAX));
            let mut colspan = 0;
            while colspan < cell.colspan && !is_taken(&taken, r, col + colspan as usize) {
                colspan += 1;
            }
            if colspan != cell.colspan {
                cell.colspan = colspan;
                cell.colwidth = None;
                changed = true;
            }
            if rowspan != cell.rowspan {
                cell.rowspan = rowspan;
                changed = true;
            }
            for row in taken.iter_mut().skip(r).take(rowspan as usize) {
                let end = col + colspan as usize;
                if row.len() < end {
                    row.resize(end, false);
                }
                row[col..end].fill(true);
            }
            col += colspan as usize;
        }
    }
    let width = taken.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    for (row, taken) in rows.iter_mut().zip(&taken) {
        let free = width - taken.iter().filter(|t| **t).count();
        if free > 0 {
            changed = true;
            let header = !row.is_empty() && row.iter().all(|cell| cell.header);
            row.extend((0..free).map(|_| TableCell {
                header,
                ..TableCell::empty()
            }));
        }
    }
    Some((rows, changed))
}
