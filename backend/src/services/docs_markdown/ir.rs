//! The Nosdesk document tree: one Rust type per node and mark in
//! `frontend/src/components/editor/schema.ts`, with the same attrs.
//!
//! Parsers build this tree, profiles and the link resolver rewrite it, the
//! normaliser makes it schema-valid, and the encoder writes it to Yjs. The
//! types already rule out some invalid shapes (a heading or a code block
//! holding block content, an image outside inline content, an embedded
//! document inside a paragraph). The rest are content rules the normaliser
//! enforces.

/// A whole page body: the `doc` node, `(block | table)+`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// The `block` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `paragraph`, `inline*`.
    Paragraph(Vec<Inline>),
    /// `heading`, `inline*`, attr `level` 1 to 6.
    Heading { level: u8, content: Vec<Inline> },
    /// `blockquote`, `block+`.
    Blockquote(Vec<Block>),
    /// `code_block`, `text*` with no marks, attr `language`.
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    /// `horizontal_rule`, a leaf.
    HorizontalRule,
    /// `bullet_list`, `list_item+`.
    BulletList(Vec<ListItem>),
    /// `ordered_list`, `list_item+`, attr `order` (the start number).
    OrderedList { order: u32, items: Vec<ListItem> },
    /// `embedded_document`, a leaf, attrs `documentUuid` and `documentTitle`.
    EmbeddedDocument {
        document_uuid: String,
        document_title: String,
    },
    /// `table`. Not in the `block` group: the schema allows tables only at
    /// the top level, so the normaliser moves any other table there.
    Table(Table),
}

/// `list_item`, `paragraph block*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// `table`, `table_row+`; each row is a `table_row`,
/// `(table_cell | table_header)*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Vec<TableCell>>,
}

/// `table_cell`, or `table_header` when `header` is set: `cell_block+`
/// (`paragraph`, `bullet_list`, `ordered_list`, `code_block`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub header: bool,
    /// Columns this cell spans, at least 1.
    pub colspan: u32,
    /// Rows this cell spans, at least 1.
    pub rowspan: u32,
    /// Width in px of each spanned column; `None` is unset.
    pub colwidth: Option<Vec<u32>>,
    pub align: Option<Alignment>,
    pub blocks: Vec<Block>,
}

impl TableCell {
    /// A one-by-one cell holding one paragraph.
    pub fn new(header: bool, inlines: Vec<Inline>) -> Self {
        TableCell {
            header,
            colspan: 1,
            rowspan: 1,
            colwidth: None,
            align: None,
            blocks: vec![Block::Paragraph(inlines)],
        }
    }

    /// An empty one-by-one body cell, as padding.
    pub fn empty() -> Self {
        TableCell::new(false, Vec::new())
    }
}

/// A table cell's `align` attr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

impl Alignment {
    pub fn as_str(self) -> &'static str {
        match self {
            Alignment::Left => "left",
            Alignment::Center => "center",
            Alignment::Right => "right",
        }
    }
}

/// The `inline` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// `text` with its marks.
    Text(Text),
    /// `image`, inline.
    Image(Image),
    /// `hard_break`.
    HardBreak,
    /// `ticket_link`, attrs `ticketId` (a string, as the editor writes it)
    /// and `href`.
    TicketLink { ticket_id: String, href: String },
    /// `mention`, attrs `uuid`, `name`, `avatarUrl`.
    Mention {
        uuid: String,
        name: String,
        avatar_url: Option<String>,
    },
}

impl Inline {
    /// Plain text with no marks.
    pub fn text(text: impl Into<String>) -> Self {
        Inline::Text(Text {
            text: text.into(),
            marks: Marks::default(),
        })
    }

    /// Text with the given marks.
    pub fn marked(text: impl Into<String>, marks: Marks) -> Self {
        Inline::Text(Text {
            text: text.into(),
            marks,
        })
    }
}

/// A run of text and its marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub marks: Marks,
}

/// The schema's marks. `ychange` is the editor's own and is never written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pub link: Option<Link>,
    pub em: bool,
    pub strong: bool,
    pub code: bool,
}

impl Marks {
    /// Only the `strong` mark.
    pub fn strong() -> Self {
        Marks {
            strong: true,
            ..Marks::default()
        }
    }

    /// Only the `code` mark.
    pub fn code() -> Self {
        Marks {
            code: true,
            ..Marks::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.link.is_none() && !self.em && !self.strong && !self.code
    }
}

/// The `link` mark's attrs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub href: String,
    pub title: Option<String>,
}

/// The `image` node's attrs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    pub src: String,
    pub alt: Option<String>,
    pub title: Option<String>,
    /// Rendered width in px; `None` is natural size.
    pub width: Option<u32>,
    pub align: Option<ImageAlign>,
}

/// The `image` node's `align` attr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAlign {
    Left,
    Center,
    Right,
}

impl ImageAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageAlign::Left => "left",
            ImageAlign::Center => "center",
            ImageAlign::Right => "right",
        }
    }
}

/// The plain text of inline content: what a reader sees, with images as
/// their alt text, mentions as `@name` and breaks as spaces.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) => out.push_str(&t.text),
            Inline::Image(img) => out.push_str(img.alt.as_deref().unwrap_or("")),
            Inline::HardBreak => out.push(' '),
            Inline::TicketLink { href, .. } => out.push_str(href),
            Inline::Mention { name, .. } => {
                out.push('@');
                out.push_str(name);
            }
        }
    }
    out
}

/// The plain text of blocks, one line per paragraph or other text block,
/// table rows as "cell · cell" with empty cells left out.
pub fn blocks_text(blocks: &[Block]) -> Vec<String> {
    let mut lines = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::Heading {
                content: inlines, ..
            } => {
                lines.push(plain_text(inlines));
            }
            Block::Blockquote(inner) => lines.extend(blocks_text(inner)),
            Block::CodeBlock { text, .. } => lines.extend(text.lines().map(str::to_string)),
            Block::HorizontalRule => {}
            Block::BulletList(items) | Block::OrderedList { items, .. } => {
                for item in items {
                    lines.extend(blocks_text(&item.blocks));
                }
            }
            Block::EmbeddedDocument { document_title, .. } => lines.push(document_title.clone()),
            Block::Table(table) => {
                for row in &table.rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|cell| blocks_text(&cell.blocks).join(" "))
                        .filter(|cell| !cell.trim().is_empty())
                        .collect();
                    lines.push(cells.join(" · "));
                }
            }
        }
    }
    lines.retain(|line| !line.trim().is_empty());
    lines
}
