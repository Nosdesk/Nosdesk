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
//!   it; tables and math are flattened into it as text).
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
//!
//! Tables have no editor node yet, so [`table_to_code_block`] lowers them.

use super::ir::{
    plain_text, Alignment, Block, Document, Image, Inline, Link, ListItem, Table, Text,
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
    };
    let mut blocks = normaliser.blocks(document.blocks, 0);
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
                let lowered = table_to_code_block(table, self.notes);
                self.block(lowered, depth, out);
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

/// A table as a `markdown` code block holding a re-aligned pipe table, with
/// inline formatting flattened to text. This is the 1.1.3 fallback until the
/// editor has a table node; a code block that parses as exactly one table is
/// the shape the upgrade recognises.
pub fn table_to_code_block(table: Table, notes: &mut Notes) -> Block {
    notes.add(NoteCode::TablesAsText);
    let columns = table
        .rows
        .iter()
        .map(Vec::len)
        .chain([table.alignments.len(), table.head.len()])
        .max()
        .unwrap_or(0)
        .max(1);
    let cell_text = |cell: Option<&Vec<Inline>>| -> String {
        cell.map(|c| single_line(&plain_text(c)).trim().replace('|', "\\|"))
            .unwrap_or_default()
    };
    let head: Vec<String> = (0..columns).map(|i| cell_text(table.head.get(i))).collect();
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| (0..columns).map(|i| cell_text(row.get(i))).collect())
        .collect();
    let alignment = |i: usize| table.alignments.get(i).copied().unwrap_or_default();

    let mut widths = vec![3usize; columns];
    for row in std::iter::once(&head).chain(rows.iter()) {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let line = |cells: &[String]| -> String {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let gap = widths[i] - cell.chars().count();
                let (left, right) = match alignment(i) {
                    Alignment::Right => (gap, 0),
                    Alignment::Center => (gap / 2, gap - gap / 2),
                    Alignment::None | Alignment::Left => (0, gap),
                };
                format!("{}{}{}", " ".repeat(left), cell, " ".repeat(right))
            })
            .collect();
        format!("| {} |", padded.join(" | "))
    };
    let rule: Vec<String> = (0..columns)
        .map(|i| {
            let w = widths[i];
            match alignment(i) {
                Alignment::None => "-".repeat(w),
                Alignment::Left => format!(":{}", "-".repeat(w - 1)),
                Alignment::Right => format!("{}:", "-".repeat(w - 1)),
                Alignment::Center => format!(":{}:", "-".repeat(w - 2)),
            }
        })
        .collect();

    let mut lines = vec![line(&head), format!("| {} |", rule.join(" | "))];
    lines.extend(rows.iter().map(|row| line(row)));
    Block::CodeBlock {
        language: Some("markdown".to_string()),
        text: lines.join("\n"),
    }
}
