//! The Nosdesk document tree: one Rust type per node and mark in
//! `frontend/src/components/editor/schema.ts`, with the same attrs.
//!
//! Parsers build this tree, profiles and the link resolver rewrite it, the
//! normaliser makes it schema-valid, and the encoder writes it to Yjs. The
//! types already rule out some invalid shapes (a heading or a code block
//! holding block content, an image outside inline content, an embedded
//! document inside a paragraph). The rest are content rules the normaliser
//! enforces.
//!
//! The one node with no editor counterpart yet is [`Block::Table`]. The
//! normaliser lowers it to a `code_block` until the editor gains a table node.

/// A whole page body: the `doc` node, `block+`.
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
    /// A table. Not a schema node yet; see the module docs.
    Table(Table),
}

/// `list_item`, `paragraph block*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// A table: an optional header row, body rows, and a column alignment each.
/// Cells hold inline content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    /// The header row's cells; empty when the table has no header.
    pub head: Vec<TableCell>,
    pub rows: Vec<Vec<TableCell>>,
}

/// One table cell's inline content.
pub type TableCell = Vec<Inline>;

/// A table column's alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    None,
    Left,
    Center,
    Right,
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
