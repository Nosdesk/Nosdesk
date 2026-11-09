//! Raw HTML in Markdown, read into the [`ir`](super::ir) tree.
//!
//! An HTML block is parsed by html5ever (the parser ammonia uses) as a
//! fragment, the way a browser would read it inside a `<div>`, and the DOM
//! is walked into IR nodes. Nothing is passed through as HTML: allowlisted
//! tags become their nodes and marks, scripts, styles, frames and comments
//! are removed with their content, and any other tag keeps only its
//! content.
//!
//! This is what reads back the HTML export v2 writes: tables a GFM pipe
//! table can't hold, and the editor's own elements for mentions, ticket
//! links, embedded pages and image widths. Whether a mention, ticket link or
//! embedded page survives is the [`Resolver`]'s call; otherwise it becomes
//! text, with a note.
//!
//! Whitespace follows the export rather than a browser: runs of spaces are
//! kept (they are text), while a run holding a line break is layout and
//! reads as one space, dropped at the edges of a block and beside a line
//! break.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use html5ever::interface::{Attribute, ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::{local_name, ns, ExpandedName, QualName};

use super::ir::{
    Alignment, Block, Document, Image, ImageAlign, Inline, Link, ListItem, Marks, Table, TableCell,
};
use super::normalise::{merge_adjacent_text, Resolver};
use super::{NoteCode, Notes};

// A small DOM for html5ever's tree builder.

type Handle = Rc<Node>;

struct Node {
    parent: Cell<Option<Weak<Node>>>,
    children: RefCell<Vec<Handle>>,
    data: Data,
}

enum Data {
    Document,
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        template: Option<Handle>,
    },
    Text(RefCell<StrTendril>),
    Comment,
    Other,
}

impl Node {
    fn new(data: Data) -> Handle {
        Rc::new(Node {
            parent: Cell::new(None),
            children: RefCell::new(Vec::new()),
            data,
        })
    }
}

fn append_child(parent: &Handle, child: Handle) {
    child.parent.set(Some(Rc::downgrade(parent)));
    parent.children.borrow_mut().push(child);
}

fn detach(target: &Handle) {
    if let Some(parent) = target.parent.take().and_then(|w| w.upgrade()) {
        parent
            .children
            .borrow_mut()
            .retain(|child| !Rc::ptr_eq(child, target));
    }
}

/// Append `text` to `node` when it is a text node.
fn extend_text(node: &Handle, text: &StrTendril) -> bool {
    match &node.data {
        Data::Text(existing) => {
            existing.borrow_mut().push_slice(text);
            true
        }
        _ => false,
    }
}

struct Dom {
    document: Handle,
}

impl Default for Dom {
    fn default() -> Self {
        Dom {
            document: Node::new(Data::Document),
        }
    }
}

impl TreeSink for Dom {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a> = ExpandedName<'a>;

    fn finish(self) -> Self {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        self.document.clone()
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> ExpandedName<'a> {
        match &target.data {
            Data::Element { name, .. } => name.expanded(),
            // The tree builder only asks for the names of elements.
            _ => unreachable!("elem_name of a non-element"),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        Node::new(Data::Element {
            name,
            attrs: RefCell::new(attrs),
            template: flags.template.then(|| Node::new(Data::Document)),
        })
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        Node::new(Data::Comment)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        Node::new(Data::Comment)
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        match child {
            NodeOrText::AppendText(text) => {
                let last = parent.children.borrow().last().cloned();
                if last.is_some_and(|last| extend_text(&last, &text)) {
                    return;
                }
                append_child(parent, Node::new(Data::Text(RefCell::new(text))));
            }
            NodeOrText::AppendNode(node) => append_child(parent, node),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        let parent = element.parent.take();
        let has_parent = parent.as_ref().is_some_and(|p| p.upgrade().is_some());
        element.parent.set(parent);
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {
        append_child(&self.document, Node::new(Data::Other));
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        match &target.data {
            Data::Element {
                template: Some(contents),
                ..
            } => contents.clone(),
            _ => Node::new(Data::Document),
        }
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        Rc::ptr_eq(x, y)
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let Some(parent) = sibling
            .parent
            .take()
            .and_then(|w| w.upgrade())
            .inspect(|p| sibling.parent.set(Some(Rc::downgrade(p))))
        else {
            return;
        };
        let index = parent
            .children
            .borrow()
            .iter()
            .position(|c| Rc::ptr_eq(c, sibling))
            .unwrap_or(0);
        let node = match child {
            NodeOrText::AppendText(text) => {
                if index > 0 {
                    let prev = parent.children.borrow()[index - 1].clone();
                    if extend_text(&prev, &text) {
                        return;
                    }
                }
                Node::new(Data::Text(RefCell::new(text)))
            }
            NodeOrText::AppendNode(node) => {
                detach(&node);
                node
            }
        };
        node.parent.set(Some(Rc::downgrade(&parent)));
        let index = parent
            .children
            .borrow()
            .iter()
            .position(|c| Rc::ptr_eq(c, sibling))
            .unwrap_or(index);
        parent.children.borrow_mut().insert(index, node);
    }

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if let Data::Element {
            attrs: existing, ..
        } = &target.data
        {
            let mut existing = existing.borrow_mut();
            for attr in attrs {
                if !existing.iter().any(|e| e.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        detach(target);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let children = std::mem::take(&mut *node.children.borrow_mut());
        for child in &children {
            child.parent.set(Some(Rc::downgrade(new_parent)));
        }
        new_parent.children.borrow_mut().extend(children);
    }
}

/// Parse `raw` as a fragment inside a `<div>`, returning the nodes it holds.
fn fragment(raw: &str) -> Vec<Handle> {
    let parser = html5ever::parse_fragment(
        Dom::default(),
        Default::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        false,
    );
    let dom = parser.one(raw);
    let html = dom.document.children.borrow().first().cloned();
    html.map(|html| html.children.borrow().clone())
        .unwrap_or_default()
}

fn tag_name(node: &Handle) -> Option<&str> {
    match &node.data {
        Data::Element { name, .. } if name.ns == ns!(html) => Some(&name.local),
        Data::Element { .. } => Some(""),
        _ => None,
    }
}

fn attr(node: &Handle, name: &str) -> Option<String> {
    match &node.data {
        Data::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| a.name.ns == ns!() && &*a.name.local == name)
            .map(|a| a.value.to_string()),
        _ => None,
    }
}

fn has_attr(node: &Handle, name: &str) -> bool {
    attr(node, name).is_some()
}

/// All the text under `node`, `<br>` as a line break.
fn text_content(node: &Handle) -> String {
    let mut out = String::new();
    fn walk(node: &Handle, out: &mut String) {
        match &node.data {
            Data::Text(text) => out.push_str(&text.borrow()),
            Data::Element { .. } => {
                let name = tag_name(node).unwrap_or("");
                if name == "br" {
                    out.push('\n');
                } else if !is_removed_with_content(name) {
                    for child in node.children.borrow().iter() {
                        walk(child, out);
                    }
                }
            }
            _ => {}
        }
    }
    walk(node, &mut out);
    out
}

// Which tags are which.

/// Tags removed together with everything inside them.
pub(crate) fn is_removed_with_content(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "iframe"
            | "frame"
            | "frameset"
            | "object"
            | "embed"
            | "applet"
            | "template"
            | "noscript"
            | "svg"
            | "math"
            | "canvas"
            | "audio"
            | "video"
            | "head"
            | "title"
            | "meta"
            | "link"
            | "base"
            | "input"
            | "select"
            | "textarea"
            | "button"
    )
}

/// Formatting the editor has no mark for; the text is kept.
pub(crate) fn is_strike_like(name: &str) -> bool {
    matches!(
        name,
        "del" | "s" | "strike" | "u" | "ins" | "mark" | "sup" | "sub"
    )
}

/// Elements that start a block of their own.
fn is_block_element(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "pre"
            | "table"
            | "hr"
            | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "nav"
            | "figure"
            | "figcaption"
            | "aside"
            | "details"
            | "summary"
            | "dl"
            | "dt"
            | "dd"
            | "center"
            | "address"
            | "form"
            | "fieldset"
            | "caption"
            | "tr"
            | "td"
            | "th"
            | "thead"
            | "tbody"
            | "tfoot"
            | "body"
            | "html"
    )
}

/// Containers that hold blocks and add nothing the editor can keep.
fn is_transparent(name: &str) -> bool {
    matches!(
        name,
        "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "nav"
            | "figure"
            | "center"
            | "address"
            | "form"
            | "fieldset"
            | "body"
            | "html"
    )
}

// Reading the DOM into blocks.

/// A container an HTML block opened and left open, so the Markdown blocks
/// after it belong inside it until a block that closes it (Notion writes
/// callouts and toggles this way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Aside,
    Details,
}

impl Container {
    fn name(self) -> &'static str {
        match self {
            Container::Aside => "aside",
            Container::Details => "details",
        }
    }
}

/// An HTML block read into blocks.
pub struct ReadBlock {
    pub blocks: Vec<Block>,
    /// The `<aside>` or `<details>` the block opened and didn't close.
    pub left_open: Option<Container>,
}

/// Read one HTML block. Markdown's HTML has no dialect: every element gets
/// the default reading.
pub fn read_block(raw: &str, resolver: &dyn Resolver, notes: &mut Notes) -> ReadBlock {
    read_block_with(raw, &DefaultDialect, resolver, notes)
}

/// [`read_block`] with a source's dialect: the HTML blocks of a source's
/// own Markdown (a BookStack callout written as `<p class="callout">`).
pub fn read_block_with(
    raw: &str,
    dialect: &dyn Dialect,
    resolver: &dyn Resolver,
    notes: &mut Notes,
) -> ReadBlock {
    let tags = tags(raw);
    if nesting(&tags) > MAX_NESTING {
        return ReadBlock {
            blocks: flat_blocks(raw, notes),
            left_open: None,
        };
    }
    let mut reader = Reader {
        resolver,
        dialect,
        notes,
        as_text: false,
    };
    let nodes = fragment(raw);
    let mut blocks = Vec::new();
    reader.blocks_of(&nodes, &mut blocks);
    if reader.as_text {
        reader.notes.add(NoteCode::HtmlAsText);
    }
    ReadBlock {
        blocks,
        left_open: left_open(&tags),
    }
}

/// Text too deep to walk (or to drop) as a tree, as flat paragraphs.
fn flat_blocks(raw: &str, notes: &mut Notes) -> Vec<Block> {
    notes.add(NoteCode::HtmlAsText);
    flat_text(raw)
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .map(|p| Block::Paragraph(vec![Inline::text(p)]))
        .collect()
}

/// Read a whole HTML page (or a fragment, which parses as one) into a
/// document, not yet normalised.
///
/// Only the content root is read: the first element the first matching
/// selector in `content` finds, in document order, else `<body>`. Inside
/// it, `dialect` classifies each element before the default reading.
pub fn read_document(
    raw: &str,
    content: &[Selector],
    dialect: &dyn Dialect,
    resolver: &dyn Resolver,
    notes: &mut Notes,
) -> Document {
    if nesting(&tags(raw)) > MAX_NESTING {
        // The page's text, chrome and all: a tree this deep isn't walked.
        return Document {
            blocks: flat_blocks(raw, notes),
        };
    }
    let dom = html5ever::parse_document(Dom::default(), Default::default()).one(raw);
    let root = content
        .iter()
        .find_map(|selector| first_match(&dom.document, selector))
        .or_else(|| first_match(&dom.document, &Selector::Tag("body")))
        .unwrap_or_else(|| dom.document.clone());
    let mut reader = Reader {
        resolver,
        dialect,
        notes,
        as_text: false,
    };
    let children = root.children.borrow().clone();
    let mut blocks = Vec::new();
    reader.blocks_of(&children, &mut blocks);
    if reader.as_text {
        reader.notes.add(NoteCode::HtmlAsText);
    }
    Document { blocks }
}

/// A whole HTML page as elements a source can look through (an export's
/// index, a page's title and breadcrumbs): its `<html>` element. `None`
/// when the page nests deeper than [`read_document`] walks.
pub fn parse_page(raw: &str) -> Option<ElementView> {
    if nesting(&tags(raw)) > MAX_NESTING {
        return None;
    }
    let dom = html5ever::parse_document(Dom::default(), Default::default()).one(raw);
    first_match(&dom.document, &Selector::Tag("html")).map(ElementView)
}

// Dialects.

/// What a source's HTML means beyond plain HTML: its macros, callouts,
/// layouts and chrome. The reader asks the dialect about each element
/// before reading it the default way.
///
/// Classes, ids and `data-*` attributes are read here to classify and
/// never reach the page: what a rewrite carries (a label, a language, a
/// link) is plain text or a destination the [`Resolver`] still decides.
/// Scripts, styles, frames, form controls and elements outside HTML (SVG,
/// MathML) are removed whatever the dialect says; for those it can only
/// say how the removal is counted, with [`Rewrite::Remove`].
pub trait Dialect {
    /// How to read `element`, or `None` for the default reading.
    fn classify(&self, element: &ElementView) -> Option<Rewrite>;
}

/// The default reading for every element: Markdown's HTML.
pub struct DefaultDialect;

impl Dialect for DefaultDialect {
    fn classify(&self, _element: &ElementView) -> Option<Rewrite> {
        None
    }
}

/// How a dialect reads one element instead of the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rewrite {
    /// A quote whose first line is `label` in bold ("Note" when empty),
    /// then the element's content.
    Callout { label: String },
    /// A code block: the text of the first `<pre>` in the element (or of
    /// the element), with `title` as a bold paragraph before it.
    Code {
        language: Option<String>,
        title: Option<String>,
    },
    /// A collapsible section shown open: `title` in bold, then the
    /// content. The dialect removes the element holding the title.
    Expand { title: Option<String> },
    /// A status label: the element's text in bold.
    Status,
    /// A Jira issue: "KEY-12: summary" (or "KEY-12") linking to `url`
    /// when it is https, else text.
    Jira {
        key: String,
        summary: Option<String>,
        url: String,
    },
    /// Removed with its content, counted under `note` when there is one
    /// (page chrome, or a title another rewrite carries, has none).
    Remove(Option<NoteCode>),
    /// The element's content in its place, counted under `note` when
    /// there is one.
    Unwrap(Option<NoteCode>),
    /// An emoji (or a name) in place of the element.
    Emoji(String),
    /// A checklist item: its content with "☐ " or "☑ " before it. The
    /// dialect removes the checkbox itself.
    Task { checked: bool },
    /// A link "Included from {title}" to `target`, which the resolver
    /// decides like any link.
    Include { target: String, title: String },
    /// Text in place of the element, counted under `note` when there is
    /// one.
    Text {
        text: String,
        note: Option<NoteCode>,
    },
    /// An image from `src`, which the resolver decides like any image,
    /// counted under `note` when there is one.
    Image {
        src: String,
        alt: Option<String>,
        width: Option<u32>,
        note: Option<NoteCode>,
    },
}

/// An element by id, by class or by tag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selector {
    Id(&'static str),
    Class(&'static str),
    Tag(&'static str),
}

impl Selector {
    fn matches(&self, node: &Handle) -> bool {
        match self {
            Selector::Id(id) => attr(node, "id").as_deref() == Some(*id),
            Selector::Class(class) => has_class(node, class),
            Selector::Tag(tag) => tag_name(node).is_some_and(|name| name == *tag),
        }
    }
}

/// The first element under `root`, in document order, that `selector`
/// matches. Iterative, so depth costs no stack.
fn first_match(root: &Handle, selector: &Selector) -> Option<Handle> {
    let mut stack: Vec<Handle> = root.children.borrow().iter().rev().cloned().collect();
    while let Some(node) = stack.pop() {
        if selector.matches(&node) {
            return Some(node);
        }
        stack.extend(node.children.borrow().iter().rev().cloned());
    }
    None
}

fn has_class(node: &Handle, class: &str) -> bool {
    attr(node, "class").is_some_and(|c| c.split_ascii_whitespace().any(|c| c == class))
}

/// One element as a [`Dialect`] sees it, read only.
#[derive(Clone)]
pub struct ElementView(Handle);

impl ElementView {
    fn of(node: &Handle) -> Option<Self> {
        matches!(node.data, Data::Element { .. }).then(|| ElementView(node.clone()))
    }

    /// The tag name, lowercase; empty outside the HTML namespace.
    pub fn tag(&self) -> &str {
        tag_name(&self.0).unwrap_or("")
    }

    pub fn attr(&self, name: &str) -> Option<String> {
        attr(&self.0, name)
    }

    pub fn id(&self) -> Option<String> {
        self.attr("id")
    }

    /// Whether the `class` attribute lists `class`.
    pub fn has_class(&self, class: &str) -> bool {
        has_class(&self.0, class)
    }

    /// The text inside, each whitespace run as one space, trimmed.
    pub fn text(&self) -> String {
        text_content(&self.0)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn matches(&self, selector: &Selector) -> bool {
        selector.matches(&self.0)
    }

    pub fn parent(&self) -> Option<ElementView> {
        let parent = self.0.parent.take();
        let up = parent.as_ref().and_then(Weak::upgrade);
        self.0.parent.set(parent);
        up.and_then(|p| ElementView::of(&p))
    }

    /// The child elements in order; text between them is skipped.
    pub fn children(&self) -> Vec<ElementView> {
        self.0
            .children
            .borrow()
            .iter()
            .filter_map(ElementView::of)
            .collect()
    }

    /// The first element inside this one, in document order, that
    /// `selector` matches.
    pub fn find(&self, selector: &Selector) -> Option<ElementView> {
        first_match(&self.0, selector).map(ElementView)
    }

    /// The nearest element around this one that `selector` matches.
    pub fn closest(&self, selector: &Selector) -> Option<ElementView> {
        let mut at = self.parent();
        while let Some(element) = at {
            if element.matches(selector) {
                return Some(element);
            }
            at = element.parent();
        }
        None
    }
}

/// Elements nested deeper than this are read as flat text: the tree walk
/// recurses, and a hostile file could nest hundreds of thousands deep.
const MAX_NESTING: usize = 100;

/// Elements that never hold content.
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
            | "param"
    )
}

/// Elements a sibling of the same kind closes, so they don't nest.
fn closes_itself(name: &str) -> bool {
    matches!(
        name,
        "p" | "li" | "td" | "th" | "tr" | "dt" | "dd" | "option"
    )
}

/// How deeply the tags open elements inside one another, counted loosely
/// (an unclosed element counts as open).
fn nesting(tags: &[HtmlTag]) -> usize {
    let mut depth: usize = 0;
    let mut deepest = 0;
    for tag in tags {
        if is_void(&tag.name) || closes_itself(&tag.name) || tag.self_closing {
            continue;
        }
        if tag.closing {
            depth = depth.saturating_sub(1);
        } else {
            depth += 1;
            deepest = deepest.max(depth);
        }
    }
    deepest
}

/// The text of `raw` with tags dropped, block tags as paragraph breaks.
fn flat_text(raw: &str) -> String {
    #[derive(Default)]
    struct TextSink {
        text: RefCell<String>,
        skipping: Cell<u32>,
    }
    impl TokenSink for TextSink {
        type Handle = ();
        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
            match token {
                Token::CharacterTokens(t) if self.skipping.get() == 0 => {
                    self.text.borrow_mut().push_str(&t);
                }
                Token::TagToken(tag) => {
                    let name = &*tag.name;
                    if is_removed_with_content(name) && !is_void(name) {
                        let n = self.skipping.get();
                        self.skipping.set(if tag.kind == TagKind::EndTag {
                            n.saturating_sub(1)
                        } else {
                            n + 1
                        });
                    } else if is_block_element(name) || name == "br" {
                        self.text.borrow_mut().push_str("\n\n");
                    }
                }
                _ => {}
            }
            TokenSinkResult::Continue
        }
    }
    let tokenizer = Tokenizer::new(TextSink::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(raw));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.text.take()
}

/// The text after a closing `</aside>` or `</details>` that starts `raw`,
/// when it does.
pub fn after_close(raw: &str, container: Container) -> Option<&str> {
    let trimmed = raw.trim_start();
    let close = format!("</{}", container.name());
    if !trimmed
        .get(..close.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(&close))
    {
        return None;
    }
    let end = trimmed.find('>')?;
    Some(&trimmed[end + 1..])
}

struct Reader<'a> {
    resolver: &'a dyn Resolver,
    dialect: &'a dyn Dialect,
    notes: &'a mut Notes,
    /// A tag was reduced to its content.
    as_text: bool,
}

/// Inline content being collected, with the marks open around it.
#[derive(Default)]
struct Run {
    inlines: Vec<Inline>,
}

impl Run {
    fn text(&mut self, text: &str, marks: &Marks) {
        let text = collapse_layout_whitespace(text);
        if !text.is_empty() {
            self.inlines.push(Inline::marked(text, marks.clone()));
        }
    }

    /// The run as a paragraph's content: layout whitespace dropped at the
    /// edges and beside line breaks. `None` when nothing but layout is left.
    fn finish(&mut self) -> Option<Vec<Inline>> {
        let mut inlines = std::mem::take(&mut self.inlines);
        trim_layout(&mut inlines);
        merge_adjacent_text(&mut inlines);
        (!inlines.is_empty()).then_some(inlines)
    }

    fn flush_into(&mut self, out: &mut Vec<Block>) {
        if let Some(inlines) = self.finish() {
            out.push(Block::Paragraph(inlines));
        }
    }
}

/// The marker for whitespace that held a line break: layout, not text.
const LAYOUT: char = '\n';

/// Text with each whitespace run that holds a line break as one [`LAYOUT`]
/// character. Other runs of spaces are text and stay.
fn collapse_layout_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.contains(['\n', '\r']) {
            out.push(LAYOUT);
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Drop layout whitespace at the edges of a run and beside line breaks.
fn trim_layout(inlines: &mut Vec<Inline>) {
    let len = inlines.len();
    for i in 0..len {
        let before_is_edge = i == 0 || matches!(inlines[i - 1], Inline::HardBreak);
        let after_is_edge = i + 1 == len || matches!(inlines[i + 1], Inline::HardBreak);
        if let Inline::Text(text) = &mut inlines[i] {
            if before_is_edge {
                text.text = text.text.trim_start_matches(LAYOUT).to_string();
            }
            if after_is_edge {
                text.text = text.text.trim_end_matches(LAYOUT).to_string();
            }
        }
    }
    inlines.retain(|i| !matches!(i, Inline::Text(t) if t.text.is_empty()));
    // A layout space beside one that a neighbour run already ends with.
    for i in 1..inlines.len() {
        let prev_ends = matches!(&inlines[i - 1], Inline::Text(t) if t.text.ends_with(LAYOUT));
        if let (true, Inline::Text(text)) = (prev_ends, &mut inlines[i]) {
            text.text = text.text.trim_start_matches(LAYOUT).to_string();
        }
    }
    inlines.retain(|i| !matches!(i, Inline::Text(t) if t.text.is_empty()));
    // Nothing but layout left between edges.
    if inlines
        .iter()
        .all(|i| matches!(i, Inline::Text(t) if t.text.chars().all(|c| c == LAYOUT)))
    {
        inlines.clear();
    }
}

impl Reader<'_> {
    /// Nodes as blocks. Inline content between blocks becomes a paragraph.
    fn blocks_of(&mut self, nodes: &[Handle], out: &mut Vec<Block>) {
        let mut run = Run::default();
        self.blocks_into(nodes, &mut run, out);
        run.flush_into(out);
    }

    /// [`Reader::blocks_of`], adding inline content to `run`, which the
    /// caller flushes.
    fn blocks_into(&mut self, nodes: &[Handle], run: &mut Run, out: &mut Vec<Block>) {
        for node in nodes {
            match &node.data {
                Data::Text(text) => run.text(&text.borrow(), &Marks::default()),
                Data::Comment => self.notes.add(NoteCode::HtmlRemoved),
                Data::Element { .. } => {
                    let name = tag_name(node).unwrap_or("");
                    if is_removed_with_content(name) || name.is_empty() {
                        self.removed(node);
                    } else if let Some(rewrite) = self.classify(node) {
                        self.rewrite_block(node, name, rewrite, run, out);
                    } else if is_block_element(name) {
                        run.flush_into(out);
                        self.block(node, name, out);
                    } else {
                        self.inline(node, run, &Marks::default());
                    }
                }
                _ => {}
            }
        }
    }

    /// The dialect's reading of an element the reader would keep.
    fn classify(&self, node: &Handle) -> Option<Rewrite> {
        ElementView::of(node).and_then(|element| self.dialect.classify(&element))
    }

    /// An element removed with its content (a script, a form control, SVG).
    /// The dialect may say how it is counted (a checklist's checkbox is
    /// not removed HTML), never bring its content back.
    fn removed(&mut self, node: &Handle) {
        match self.classify(node) {
            Some(Rewrite::Remove(note)) => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
            }
            _ => self.notes.add(NoteCode::HtmlRemoved),
        }
    }

    /// A rewrite where blocks may stand. A block rewrite ends the paragraph
    /// `run` was collecting; an inline one adds to it.
    fn rewrite_block(
        &mut self,
        node: &Handle,
        name: &str,
        rewrite: Rewrite,
        run: &mut Run,
        out: &mut Vec<Block>,
    ) {
        match rewrite {
            Rewrite::Callout { label } => {
                run.flush_into(out);
                self.notes.add(NoteCode::CalloutsAsQuotes);
                let mut blocks = self.children_blocks(node);
                let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                let label = if label.is_empty() {
                    "Note".to_string()
                } else {
                    label
                };
                blocks.insert(0, bold_paragraph(label));
                out.push(Block::Blockquote(blocks));
            }
            Rewrite::Code { language, title } => {
                run.flush_into(out);
                if let Some(title) = non_empty(title) {
                    out.push(bold_paragraph(title));
                }
                out.push(Block::CodeBlock {
                    language,
                    text: code_text(node),
                });
            }
            Rewrite::Expand { title } => {
                run.flush_into(out);
                self.notes.add(NoteCode::ExpandsShownOpen);
                if let Some(title) = non_empty(title) {
                    out.push(bold_paragraph(title));
                }
                out.extend(self.children_blocks(node));
            }
            Rewrite::Task { checked } => {
                run.flush_into(out);
                self.notes.add(NoteCode::ChecklistsAsBullets);
                out.extend(self.task_blocks(node, checked));
            }
            Rewrite::Unwrap(note) => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
                let children = node.children.borrow().clone();
                if is_block_element(name) {
                    run.flush_into(out);
                    self.blocks_of(&children, out);
                } else {
                    self.blocks_into(&children, run, out);
                }
            }
            Rewrite::Remove(note) => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
            }
            inline => self.rewrite_inline(node, inline, run, &Marks::default()),
        }
    }

    /// A rewrite inside a paragraph. A block rewrite there keeps its
    /// content as text.
    fn rewrite_inline(&mut self, node: &Handle, rewrite: Rewrite, run: &mut Run, marks: &Marks) {
        match rewrite {
            Rewrite::Status => {
                self.notes.add(NoteCode::StatusAsText);
                let text = text_content(node)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut bold = marks.clone();
                bold.strong = true;
                push_text(run, text, bold);
            }
            Rewrite::Jira { key, summary, url } => {
                let text = match non_empty(summary) {
                    Some(summary) => format!("{key}: {summary}"),
                    None => key,
                };
                let mut linked = marks.clone();
                if is_https(&url) {
                    self.notes.add(NoteCode::JiraAsLinks);
                    linked.link = Some(Link {
                        href: url.trim().to_string(),
                        title: None,
                    });
                } else {
                    self.notes.add(NoteCode::LinksDropped);
                }
                push_text(run, text, linked);
            }
            Rewrite::Emoji(emoji) => {
                self.notes.add(NoteCode::EmojiAsText);
                push_text(run, emoji, marks.clone());
            }
            Rewrite::Text { text, note } => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
                push_text(run, text, marks.clone());
            }
            Rewrite::Include { target, title } => {
                // Counted when the target resolves; otherwise the
                // normaliser counts the link it drops.
                if self.resolver.link(&target).is_some() {
                    self.notes.add(NoteCode::IncludesAsLinks);
                }
                let mut linked = marks.clone();
                linked.link = Some(Link {
                    href: target,
                    title: None,
                });
                push_text(run, format!("Included from {}", title.trim()), linked);
            }
            Rewrite::Image {
                src,
                alt,
                width,
                note,
            } => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
                run.inlines.push(Inline::Image(Image {
                    src,
                    alt: non_empty(alt),
                    title: None,
                    width: width.filter(|w| *w > 0 && *w <= 10_000),
                    align: None,
                }));
            }
            Rewrite::Code { .. } => {
                let mut code = marks.clone();
                code.code = true;
                push_text(run, code_text(node), code);
            }
            Rewrite::Task { checked } => {
                self.notes.add(NoteCode::ChecklistsAsBullets);
                run.inlines
                    .push(Inline::marked(task_box(checked), plain(marks)));
                self.inline_children(node, run, marks);
            }
            Rewrite::Unwrap(note) => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
                self.inline_children(node, run, marks);
            }
            Rewrite::Remove(note) => {
                if let Some(note) = note {
                    self.notes.add(note);
                }
            }
            Rewrite::Callout { .. } | Rewrite::Expand { .. } => {
                self.as_text = true;
                self.inline_children(node, run, marks);
            }
        }
    }

    /// A checklist item's blocks, its box before the first paragraph.
    fn task_blocks(&mut self, node: &Handle, checked: bool) -> Vec<Block> {
        let mut blocks = self.children_blocks(node);
        let mark = task_box(checked);
        match blocks.first_mut() {
            Some(Block::Paragraph(inlines)) => {
                // The space the checkbox left before the text.
                if let Some(Inline::Text(first)) = inlines.first_mut() {
                    first.text = first.text.trim_start().to_string();
                    if first.text.is_empty() {
                        inlines.remove(0);
                    }
                }
                let mark = if inlines.is_empty() {
                    mark.trim_end()
                } else {
                    mark
                };
                inlines.insert(0, Inline::text(mark));
                merge_adjacent_text(inlines);
            }
            _ => blocks.insert(0, Block::Paragraph(vec![Inline::text(mark.trim_end())])),
        }
        blocks
    }

    fn children_blocks(&mut self, node: &Handle) -> Vec<Block> {
        let children = node.children.borrow().clone();
        let mut out = Vec::new();
        self.blocks_of(&children, &mut out);
        out
    }

    fn children_inlines(&mut self, node: &Handle, marks: &Marks) -> Vec<Inline> {
        let mut run = Run::default();
        let children = node.children.borrow().clone();
        for child in &children {
            self.inline_node(child, &mut run, marks);
        }
        run.finish().unwrap_or_default()
    }

    fn block(&mut self, node: &Handle, name: &str, out: &mut Vec<Block>) {
        match name {
            "p" => {
                let inlines = self.children_inlines(node, &Marks::default());
                out.push(Block::Paragraph(inlines));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<u8>().unwrap_or(1);
                let content = self.children_inlines(node, &Marks::default());
                out.push(Block::Heading { level, content });
            }
            "ul" | "ol" => {
                let items = self.list_items(node);
                if name == "ol" {
                    let order = attr(node, "start")
                        .and_then(|s| s.trim().parse::<u32>().ok())
                        .unwrap_or(1);
                    out.push(Block::OrderedList { order, items });
                } else {
                    out.push(Block::BulletList(items));
                }
            }
            "li" | "dd" => out.extend(self.children_blocks(node)),
            "blockquote" => out.push(Block::Blockquote(self.children_blocks(node))),
            "pre" => out.push(code_block(node)),
            "table" => self.table(node, out),
            "hr" => out.push(Block::HorizontalRule),
            "aside" => out.push(self.callout(node)),
            "details" => out.extend(self.children_blocks(node)),
            "summary" | "dt" | "figcaption" | "caption" => {
                let bold = if name == "figcaption" || name == "caption" {
                    Marks::default()
                } else {
                    Marks::strong()
                };
                let inlines = self.children_inlines(node, &bold);
                if !inlines.is_empty() {
                    out.push(Block::Paragraph(inlines));
                }
            }
            "dl" => out.extend(self.children_blocks(node)),
            "div" if has_attr(node, "data-embedded-document") => out.push(self.embed(node)),
            "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" => {
                // Table parts outside a table: their content.
                self.as_text = true;
                out.extend(self.children_blocks(node));
            }
            _ => {
                if is_transparent(name) && name != "body" && name != "html" {
                    self.as_text = true;
                }
                out.extend(self.children_blocks(node));
            }
        }
    }

    fn list_items(&mut self, list: &Handle) -> Vec<ListItem> {
        let mut items: Vec<ListItem> = Vec::new();
        let mut stray: Vec<Handle> = Vec::new();
        // Counted once per list, as for a Markdown task list.
        let mut tasks = false;
        let children = list.children.borrow().clone();
        for child in &children {
            if tag_name(child) == Some("li") {
                if !stray.is_empty() {
                    let mut blocks = Vec::new();
                    self.blocks_of(&std::mem::take(&mut stray), &mut blocks);
                    if !blocks.is_empty() {
                        items.push(ListItem { blocks });
                    }
                }
                match self.classify(child) {
                    None => items.push(ListItem {
                        blocks: self.children_blocks(child),
                    }),
                    Some(Rewrite::Task { checked }) => {
                        tasks = true;
                        items.push(ListItem {
                            blocks: self.task_blocks(child, checked),
                        });
                    }
                    Some(rewrite) => {
                        let mut blocks = Vec::new();
                        let mut run = Run::default();
                        self.rewrite_block(child, "li", rewrite, &mut run, &mut blocks);
                        run.flush_into(&mut blocks);
                        if !blocks.is_empty() {
                            items.push(ListItem { blocks });
                        }
                    }
                }
            } else {
                stray.push(child.clone());
            }
        }
        if !stray.is_empty() {
            let mut blocks = Vec::new();
            self.blocks_of(&stray, &mut blocks);
            if !blocks.is_empty() {
                items.push(ListItem { blocks });
            }
        }
        if tasks {
            self.notes.add(NoteCode::ChecklistsAsBullets);
        }
        items
    }

    fn table(&mut self, node: &Handle, out: &mut Vec<Block>) {
        let mut rows: Vec<Vec<TableCell>> = Vec::new();
        let mut captions: Vec<Block> = Vec::new();
        let mut sections = vec![node.clone()];
        while let Some(section) = sections.pop() {
            let children = section.children.borrow().clone();
            let mut nested = Vec::new();
            for child in &children {
                match tag_name(child) {
                    Some("thead" | "tbody" | "tfoot") => nested.push(child.clone()),
                    Some("tr") => rows.push(self.row(child)),
                    Some("caption") => self.block(child, "caption", &mut captions),
                    _ => {}
                }
            }
            // Sections in document order.
            sections.extend(nested.into_iter().rev());
        }
        out.extend(captions);
        if !rows.is_empty() {
            out.push(Block::Table(Table { rows }));
        }
    }

    fn row(&mut self, tr: &Handle) -> Vec<TableCell> {
        let children = tr.children.borrow().clone();
        children
            .iter()
            .filter_map(|cell| {
                let name = tag_name(cell)?;
                if name != "td" && name != "th" {
                    return None;
                }
                let span = |name: &str| {
                    attr(cell, name)
                        .and_then(|s| s.trim().parse::<u32>().ok())
                        .unwrap_or(1)
                        .clamp(1, 1000)
                };
                let align = attr(cell, "align")
                    .or_else(|| style_value(&attr(cell, "style")?, "text-align"))
                    .and_then(|a| match a.trim().to_ascii_lowercase().as_str() {
                        "left" => Some(Alignment::Left),
                        "center" => Some(Alignment::Center),
                        "right" => Some(Alignment::Right),
                        _ => None,
                    });
                Some(TableCell {
                    header: name == "th",
                    colspan: span("colspan"),
                    rowspan: span("rowspan"),
                    colwidth: None,
                    align,
                    blocks: self.children_blocks(cell),
                })
            })
            .collect()
    }

    /// `<aside>`, as Notion writes a callout: a quote whose first paragraph
    /// is its label in bold, the callout's emoji when it opens with one.
    fn callout(&mut self, node: &Handle) -> Block {
        self.notes.add(NoteCode::CalloutsAsQuotes);
        let mut blocks = self.children_blocks(node);
        let mut label = "Note".to_string();
        if let Some(Block::Paragraph(inlines)) = blocks.first_mut() {
            if let Some(Inline::Text(first)) = inlines.first_mut() {
                if let Some(emoji) = leading_emoji(&first.text) {
                    label = emoji.to_string();
                    first.text = first.text[emoji.len()..].trim_start().to_string();
                    if first.text.is_empty() {
                        inlines.remove(0);
                    }
                }
            }
            if inlines.is_empty() {
                blocks.remove(0);
            }
        }
        blocks.insert(
            0,
            Block::Paragraph(vec![Inline::marked(label, Marks::strong())]),
        );
        Block::Blockquote(blocks)
    }

    /// `div[data-embedded-document]`: the embedded page, or its title.
    fn embed(&mut self, node: &Handle) -> Block {
        let uuid = attr(node, "data-document-uuid").unwrap_or_default();
        let title = attr(node, "data-document-title")
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| text_content(node).trim().to_string());
        match self.resolver.embed(&uuid) {
            Some(document_uuid) => Block::EmbeddedDocument {
                document_uuid,
                document_title: title,
            },
            None => {
                self.notes.add(NoteCode::EmbedsAsText);
                Block::Paragraph(vec![Inline::text(title)])
            }
        }
    }

    fn inline(&mut self, node: &Handle, run: &mut Run, marks: &Marks) {
        self.inline_node(node, run, marks);
    }

    fn inline_node(&mut self, node: &Handle, run: &mut Run, marks: &Marks) {
        match &node.data {
            Data::Text(text) => run.text(&text.borrow(), marks),
            Data::Comment => self.notes.add(NoteCode::HtmlRemoved),
            Data::Element { .. } => {
                let name = tag_name(node).unwrap_or("");
                if name.is_empty() || is_removed_with_content(name) {
                    self.removed(node);
                } else if let Some(rewrite) = self.classify(node) {
                    self.rewrite_inline(node, rewrite, run, marks);
                } else {
                    self.inline_element(node, name, run, marks);
                }
            }
            _ => {}
        }
    }

    fn inline_children(&mut self, node: &Handle, run: &mut Run, marks: &Marks) {
        let children = node.children.borrow().clone();
        for child in &children {
            self.inline_node(child, run, marks);
        }
    }

    fn inline_element(&mut self, node: &Handle, name: &str, run: &mut Run, marks: &Marks) {
        if name.is_empty() || is_removed_with_content(name) {
            self.notes.add(NoteCode::HtmlRemoved);
            return;
        }
        let mut inner = marks.clone();
        match name {
            "br" => run.inlines.push(Inline::HardBreak),
            "img" => self.image(node, run),
            "strong" | "b" => {
                inner.strong = true;
                self.inline_children(node, run, &inner);
            }
            "em" | "i" => {
                inner.em = true;
                self.inline_children(node, run, &inner);
            }
            "code" | "kbd" | "samp" | "tt" => {
                inner.code = true;
                self.inline_children(node, run, &inner);
            }
            "a" => {
                if let Some(href) = attr(node, "href") {
                    inner.link = Some(Link {
                        href,
                        title: attr(node, "title").filter(|t| !t.is_empty()),
                    });
                }
                self.inline_children(node, run, &inner);
            }
            "span" if has_attr(node, "data-mention") => self.mention(node, run, marks),
            "span" if has_attr(node, "data-ticket-link") => self.ticket(node, run, marks),
            name if is_strike_like(name) => {
                self.notes.add(NoteCode::StrikethroughRemoved);
                self.inline_children(node, run, &inner);
            }
            _ => {
                self.as_text = true;
                self.inline_children(node, run, &inner);
            }
        }
    }

    fn image(&mut self, node: &Handle, run: &mut Run) {
        let src = attr(node, "src").unwrap_or_default();
        run.inlines.push(Inline::Image(Image {
            src,
            alt: attr(node, "alt").filter(|a| !a.is_empty()),
            title: attr(node, "title").filter(|t| !t.is_empty()),
            width: attr(node, "width").and_then(|w| parse_width(&w)),
            align: attr(node, "data-align").and_then(|a| parse_image_align(&a)),
        }));
    }

    fn mention(&mut self, node: &Handle, run: &mut Run, marks: &Marks) {
        let uuid = attr(node, "data-uuid").unwrap_or_default();
        let name = attr(node, "data-name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                text_content(node)
                    .trim()
                    .trim_start_matches('@')
                    .to_string()
            });
        if !uuid.trim().is_empty() && self.resolver.mention(&uuid) {
            run.inlines.push(Inline::Mention {
                uuid,
                name,
                avatar_url: attr(node, "data-avatar-url").filter(|u| is_local_path(u)),
            });
        } else {
            self.notes.add(NoteCode::MentionsAsText);
            run.inlines
                .push(Inline::marked(format!("@{name}"), plain(marks)));
        }
    }

    fn ticket(&mut self, node: &Handle, run: &mut Run, marks: &Marks) {
        let ticket_id = attr(node, "data-ticket-id").unwrap_or_default();
        let href = attr(node, "data-href").unwrap_or_default();
        match self.resolver.ticket(&ticket_id, &href) {
            Some(href) => run.inlines.push(Inline::TicketLink { ticket_id, href }),
            None => {
                self.notes.add(NoteCode::TicketLinksAsText);
                let label = text_content(node).trim().to_string();
                let label = if label.is_empty() {
                    "Ticket".to_string()
                } else {
                    label
                };
                run.inlines.push(Inline::marked(label, plain(marks)));
            }
        }
    }
}

/// Marks for text standing in for a node: the node carried none of its own.
fn plain(marks: &Marks) -> Marks {
    Marks {
        link: marks.link.clone(),
        ..Marks::default()
    }
}

/// `text` in `run` with `marks`, each whitespace run as one space; nothing
/// when that leaves it empty.
fn push_text(run: &mut Run, text: String, marks: Marks) {
    let text = if text.contains(char::is_whitespace) {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        text
    };
    if !text.is_empty() {
        run.inlines.push(Inline::marked(text, marks));
    }
}

fn bold_paragraph(text: String) -> Block {
    Block::Paragraph(vec![Inline::marked(text, Marks::strong())])
}

/// `text` trimmed, when anything is left.
fn non_empty(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// A checklist item's box, with the space after it.
fn task_box(checked: bool) -> &'static str {
    if checked {
        "\u{2611} "
    } else {
        "\u{2610} "
    }
}

/// The text of the first `<pre>` in `node` (or `node` itself), exactly.
fn code_text(node: &Handle) -> String {
    if tag_name(node) == Some("pre") {
        return text_content(node);
    }
    match first_match(node, &Selector::Tag("pre")) {
        Some(pre) => text_content(&pre),
        None => text_content(node),
    }
}

/// An absolute https URL with a host.
fn is_https(url: &str) -> bool {
    let url = url.trim();
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && url[8..]
            .chars()
            .next()
            .is_some_and(|c| !matches!(c, '/' | '\\' | '?' | '#'))
}

/// A path on this server: one slash, not two.
fn is_local_path(url: &str) -> bool {
    url.starts_with('/') && !url.starts_with("//")
}

fn code_block(pre: &Handle) -> Block {
    let code = pre
        .children
        .borrow()
        .iter()
        .find(|c| tag_name(c) == Some("code"))
        .cloned();
    let language = code
        .as_ref()
        .and_then(|c| attr(c, "class"))
        .or_else(|| attr(pre, "class"))
        .and_then(|class| {
            class.split_whitespace().find_map(|c| {
                c.strip_prefix("language-")
                    .or_else(|| c.strip_prefix("lang-"))
                    .map(str::to_string)
            })
        });
    // The text exactly: the export writes a trailing line break only when
    // the code has one.
    Block::CodeBlock {
        language,
        text: text_content(pre),
    }
}

/// A width in px: `240`, `240px`. Percentages and zero are no width.
pub fn parse_width(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = value.strip_suffix("px").unwrap_or(value).trim();
    digits
        .parse::<u32>()
        .ok()
        .filter(|w| *w > 0 && *w <= 10_000)
}

fn parse_image_align(value: &str) -> Option<ImageAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(ImageAlign::Left),
        "center" => Some(ImageAlign::Center),
        "right" => Some(ImageAlign::Right),
        _ => None,
    }
}

/// One declaration's value from a `style` attribute.
fn style_value(style: &str, property: &str) -> Option<String> {
    style.split(';').find_map(|decl| {
        let (name, value) = decl.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(property)
            .then(|| value.trim().to_string())
    })
}

/// The emoji a callout's text opens with: a symbol that isn't a letter,
/// digit or ASCII, with the joiners and selectors that follow it.
fn leading_emoji(text: &str) -> Option<&str> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if first.is_ascii() || first.is_alphanumeric() {
        return None;
    }
    let mut end = first.len_utf8();
    let mut joined = false;
    for (i, c) in chars {
        let continues = joined
            || matches!(c, '\u{200d}' | '\u{fe0f}' | '\u{fe0e}' | '\u{20e3}')
            || ('\u{1f3fb}'..='\u{1f3ff}').contains(&c);
        if !continues {
            break;
        }
        joined = c == '\u{200d}';
        end = i + c.len_utf8();
    }
    Some(&text[..end])
}

// Single tags, for inline HTML.

/// One tag read from inline HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlTag {
    pub name: String,
    pub closing: bool,
    pub self_closing: bool,
    pub attrs: Vec<(String, String)>,
}

impl HtmlTag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Collects the tags a tokenizer reads.
#[derive(Default)]
struct TagSink {
    tags: RefCell<Vec<HtmlTag>>,
}

impl TokenSink for TagSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        if let Token::TagToken(tag) = token {
            self.tags.borrow_mut().push(HtmlTag {
                name: tag.name.to_string(),
                closing: tag.kind == TagKind::EndTag,
                self_closing: tag.self_closing,
                attrs: tag
                    .attrs
                    .iter()
                    .map(|a| (a.name.local.to_string(), a.value.to_string()))
                    .collect(),
            });
        }
        TokenSinkResult::Continue
    }
}

/// Every tag in `raw`, with attribute values decoded.
fn tags(raw: &str) -> Vec<HtmlTag> {
    let tokenizer = Tokenizer::new(TagSink::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(raw));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.tags.take()
}

/// The first tag in `raw`, as inline HTML holds one.
pub fn read_tag(raw: &str) -> Option<HtmlTag> {
    tags(raw).into_iter().next()
}

/// The `<aside>` or `<details>` the tags open and leave open, if any.
fn left_open(tags: &[HtmlTag]) -> Option<Container> {
    let mut open: Vec<Container> = Vec::new();
    for tag in tags {
        let container = match tag.name.as_str() {
            "aside" => Container::Aside,
            "details" => Container::Details,
            _ => continue,
        };
        if tag.closing {
            if let Some(i) = open.iter().rposition(|c| *c == container) {
                open.truncate(i);
            }
        } else if !tag.self_closing {
            open.push(container);
        }
    }
    open.first().copied()
}

#[cfg(test)]
mod tests {
    use super::super::ir::Text;
    use super::super::DefaultResolver;
    use super::*;

    fn read(raw: &str) -> (Vec<Block>, Notes) {
        let mut notes = Notes::default();
        let blocks = read_block(raw, &DefaultResolver, &mut notes).blocks;
        (blocks, notes)
    }

    #[test]
    fn blocks_keep_their_text_and_scripts_go() {
        let (blocks, notes) =
            read("<div>One &amp; <b>two</b></div><script>alert(1)</script><p>Three</p><!-- c -->");
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(vec![
                    Inline::text("One & "),
                    Inline::marked("two", Marks::strong())
                ]),
                Block::Paragraph(vec![Inline::text("Three")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 2);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 1);
    }

    #[test]
    fn a_comment_alone_is_nothing() {
        let (blocks, notes) = read("<!-- hidden -->\n");
        assert!(blocks.is_empty());
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 1);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    }

    #[test]
    fn line_breaks_alone_are_a_paragraph_of_breaks() {
        let (blocks, _) = read("<br>\n<br>\n");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(vec![Inline::HardBreak, Inline::HardBreak])]
        );
    }

    #[test]
    fn layout_whitespace_collapses_and_spaces_stay() {
        let (blocks, _) = read("<p>\n  Hello\n  world  <b>x</b>\n</p>");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(vec![
                Inline::text("Hello\nworld  "),
                Inline::marked("x", Marks::strong()),
            ])]
        );
    }

    #[test]
    fn an_image_keeps_its_width_and_align() {
        let (blocks, _) =
            read("<img src=\"/a.png\" alt=\"A\" width=\"240\" data-align=\"center\">\n");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(vec![Inline::Image(Image {
                src: "/a.png".into(),
                alt: Some("A".into()),
                title: None,
                width: Some(240),
                align: Some(ImageAlign::Center),
            })])]
        );
    }

    #[test]
    fn spans_become_text_when_the_resolver_says_so() {
        let (blocks, notes) = read(
            "<p><span data-mention=\"true\" data-uuid=\"u\" data-name=\"Ana\">@Ana</span> and \
             <span data-ticket-link=\"true\" data-ticket-id=\"4\" data-href=\"/tickets/4\">Ticket #9</span></p>\
             <div data-embedded-document=\"true\" data-document-uuid=\"d\" data-document-title=\"VPN\">VPN</div>",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(vec![Inline::text("@Ana and Ticket #9")]),
                Block::Paragraph(vec![Inline::text("VPN")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::MentionsAsText), 1);
        assert_eq!(notes.count(NoteCode::TicketLinksAsText), 1);
        assert_eq!(notes.count(NoteCode::EmbedsAsText), 1);
    }

    #[test]
    fn tables_keep_spans_headers_and_alignment() {
        let (blocks, _) = read(
            "<table>\n<tr><th colspan=\"2\" align=\"center\"><p>wide</p></th></tr>\n\
             <tr><th><p>side</p></th><td style=\"text-align: right\"><ul><li><p>x</p></li></ul></td></tr>\n</table>",
        );
        let Some(Block::Table(table)) = blocks.first() else {
            panic!("a table: {blocks:?}");
        };
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0].colspan, 2);
        assert_eq!(table.rows[0][0].align, Some(Alignment::Center));
        assert!(table.rows[1][0].header);
        assert_eq!(table.rows[1][1].align, Some(Alignment::Right));
        assert!(matches!(table.rows[1][1].blocks[0], Block::BulletList(_)));
    }

    #[test]
    fn notion_asides_are_callouts_with_their_emoji() {
        let (blocks, notes) = read("<aside>\n💡 Restart first.\n</aside>");
        assert_eq!(
            blocks,
            vec![Block::Blockquote(vec![
                Block::Paragraph(vec![Inline::marked("💡", Marks::strong())]),
                Block::Paragraph(vec![Inline::Text(Text {
                    text: "Restart first.".into(),
                    marks: Marks::default()
                })]),
            ])]
        );
        assert_eq!(notes.count(NoteCode::CalloutsAsQuotes), 1);
        assert_eq!(left_open(&tags("<aside>\n💡 x\n")), Some(Container::Aside));
        assert_eq!(left_open(&tags("<aside>x</aside>")), None);
        assert_eq!(after_close("</asidé", Container::Aside), None);
        assert_eq!(
            after_close("</aside>\nrest", Container::Aside),
            Some("\nrest")
        );
    }

    #[test]
    fn a_page_parses_into_elements_unless_too_deep() {
        let page = parse_page(
            "<html><head><title>QA (Space)</title></head><body><h1>QA (Space)</h1><ol id=\"crumbs\">\
             <li><a href=\"a.html\">A &amp; B</a></li><li><a href=\"b.html\">B</a></li></ol></body></html>",
        )
        .expect("parses");
        assert_eq!(page.tag(), "html");
        assert_eq!(
            page.find(&Selector::Tag("h1")).unwrap().text(),
            "QA (Space)"
        );
        let crumbs = page.find(&Selector::Id("crumbs")).unwrap();
        let links: Vec<(String, Option<String>)> = crumbs
            .children()
            .iter()
            .filter_map(|li| li.find(&Selector::Tag("a")))
            .map(|a| (a.text(), a.attr("href")))
            .collect();
        assert_eq!(
            links,
            [
                ("A & B".to_string(), Some("a.html".to_string())),
                ("B".to_string(), Some("b.html".to_string()))
            ]
        );
        assert!(parse_page(&"<div>".repeat(500)).is_none());
    }

    #[test]
    fn very_deep_html_is_read_as_flat_text() {
        let raw = format!("{}deep{}", "<div>".repeat(50_000), "</div>".repeat(10));
        let (blocks, notes) = read(&raw);
        assert_eq!(blocks, vec![Block::Paragraph(vec![Inline::text("deep")])]);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 1);
        let (blocks, _) = read(&"<p>a".repeat(5_000));
        assert_eq!(
            blocks.len(),
            5_000,
            "siblings that close each other are not deep"
        );
    }

    /// A dialect with one rule per rewrite, by class.
    struct TestDialect;

    impl Dialect for TestDialect {
        fn classify(&self, el: &ElementView) -> Option<Rewrite> {
            let class = |c: &str| el.has_class(c);
            if el.tag() == "input" && el.closest(&Selector::Class("task")).is_some() {
                return Some(Rewrite::Remove(None));
            }
            if class("title") && el.closest(&Selector::Class("callout")).is_some() {
                return Some(Rewrite::Remove(None));
            }
            if class("callout") {
                let label = el
                    .find(&Selector::Class("title"))
                    .map(|t| t.text())
                    .unwrap_or_default();
                return Some(Rewrite::Callout { label });
            }
            if class("code-title") || class("expand-title") {
                return Some(Rewrite::Remove(None));
            }
            if class("code") {
                return Some(Rewrite::Code {
                    language: el.attr("data-lang"),
                    title: el.find(&Selector::Class("code-title")).map(|t| t.text()),
                });
            }
            if class("expand") {
                return Some(Rewrite::Expand {
                    title: el.find(&Selector::Class("expand-title")).map(|t| t.text()),
                });
            }
            if class("status") {
                return Some(Rewrite::Status);
            }
            if class("jira") {
                return Some(Rewrite::Jira {
                    key: el.attr("data-key").unwrap_or_default(),
                    summary: el.find(&Selector::Class("summary")).map(|s| s.text()),
                    url: el.attr("data-url").unwrap_or_default(),
                });
            }
            if class("toc") {
                return Some(Rewrite::Remove(Some(NoteCode::MacrosRemoved)));
            }
            if class("layout") {
                return Some(Rewrite::Unwrap(Some(NoteCode::LayoutFlattened)));
            }
            if class("wrap") {
                return Some(Rewrite::Unwrap(None));
            }
            if class("emoticon") {
                return Some(match el.attr("data-name").as_deref() {
                    Some("tick") => Rewrite::Emoji("\u{2705}".into()),
                    _ => Rewrite::Text {
                        text: el.attr("alt").unwrap_or_default(),
                        note: Some(NoteCode::EmojiAsText),
                    },
                });
            }
            if class("task") {
                return Some(Rewrite::Task {
                    checked: class("checked"),
                });
            }
            if class("drawio") {
                return Some(Rewrite::Unwrap(Some(NoteCode::DiagramsAsImages)));
            }
            if class("gliffy") {
                return Some(Rewrite::Text {
                    text: el.attr("data-name").unwrap_or_default(),
                    note: Some(NoteCode::DiagramsLeftOut),
                });
            }
            if class("include") {
                return Some(Rewrite::Include {
                    target: el.attr("data-href").unwrap_or_default(),
                    title: el.text(),
                });
            }
            if class("attached") {
                return Some(Rewrite::Image {
                    src: el.attr("data-original").unwrap_or_default(),
                    alt: el.attr("data-alias"),
                    width: el.attr("width").and_then(|w| parse_width(&w)),
                    note: None,
                });
            }
            None
        }
    }

    fn page(raw: &str) -> (Vec<Block>, Notes) {
        let mut notes = Notes::default();
        let document = read_document(raw, &[], &TestDialect, &DefaultResolver, &mut notes);
        (document.blocks, notes)
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(inlines)
    }

    fn bold(text: &str) -> Inline {
        Inline::marked(text, Marks::strong())
    }

    fn linked(text: &str, href: &str) -> Inline {
        Inline::marked(
            text,
            Marks {
                link: Some(Link {
                    href: href.into(),
                    title: None,
                }),
                ..Marks::default()
            },
        )
    }

    #[test]
    fn a_callout_is_a_quote_led_by_its_label() {
        let (blocks, notes) = page(
            "<div class=\"callout\"><p class=\"title\">Heads up</p><p>Restart first.</p></div>\
             <div class=\"callout\"><p>No title.</p></div>",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Blockquote(vec![
                    para(vec![bold("Heads up")]),
                    para(vec![Inline::text("Restart first.")]),
                ]),
                Block::Blockquote(vec![
                    para(vec![bold("Note")]),
                    para(vec![Inline::text("No title.")]),
                ]),
            ]
        );
        assert_eq!(notes.count(NoteCode::CalloutsAsQuotes), 2);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    }

    #[test]
    fn a_code_macro_is_a_code_block_with_its_title() {
        let (blocks, _) = page(
            "<div class=\"code\" data-lang=\"java\"><div class=\"code-title\">Main.java</div>\
             <div><pre>class A {}\n</pre></div></div>",
        );
        assert_eq!(
            blocks,
            vec![
                para(vec![bold("Main.java")]),
                Block::CodeBlock {
                    language: Some("java".into()),
                    text: "class A {}\n".into(),
                },
            ]
        );
    }

    #[test]
    fn an_expand_is_shown_open_with_its_title() {
        let (blocks, notes) = page(
            "<div class=\"expand\"><span class=\"expand-title\">More</span>\
             <div class=\"wrap\"><p>Hidden text.</p></div></div>",
        );
        assert_eq!(
            blocks,
            vec![
                para(vec![bold("More")]),
                para(vec![Inline::text("Hidden text.")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::ExpandsShownOpen), 1);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    }

    #[test]
    fn a_status_is_bold_text() {
        let (blocks, notes) = page("<p>State: <span class=\"status\"> In\n progress </span></p>");
        assert_eq!(
            blocks,
            vec![para(vec![Inline::text("State: "), bold("In progress")])]
        );
        assert_eq!(notes.count(NoteCode::StatusAsText), 1);
    }

    #[test]
    fn a_jira_issue_links_only_over_https() {
        let (blocks, notes) = page(
            "<p><span class=\"jira\" data-key=\"IT-12\" data-url=\"https://acme.atlassian.net/browse/IT-12\">\
             IT-12 <span class=\"summary\">VPN drops</span></span> and \
             <span class=\"jira\" data-key=\"IT-13\" data-url=\"javascript:alert(1)\">IT-13</span></p>",
        );
        assert_eq!(
            blocks,
            vec![para(vec![
                linked(
                    "IT-12: VPN drops",
                    "https://acme.atlassian.net/browse/IT-12"
                ),
                Inline::text(" and IT-13"),
            ])]
        );
        assert_eq!(notes.count(NoteCode::JiraAsLinks), 1);
        assert_eq!(notes.count(NoteCode::LinksDropped), 1);
    }

    #[test]
    fn removed_macros_and_flattened_layouts_are_counted() {
        let (blocks, notes) = page(
            "<div class=\"toc\"><ul><li>Intro</li></ul></div>\
             <div class=\"layout\"><div class=\"wrap\"><p>Left</p></div><div class=\"wrap\"><p>Right</p></div></div>\
             <p>a<span class=\"wrap\">b</span>c</p>",
        );
        assert_eq!(
            blocks,
            vec![
                para(vec![Inline::text("Left")]),
                para(vec![Inline::text("Right")]),
                para(vec![Inline::text("abc")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::MacrosRemoved), 1);
        assert_eq!(notes.count(NoteCode::LayoutFlattened), 1);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 0);
    }

    #[test]
    fn emoticons_become_emoji_or_their_name() {
        let (blocks, notes) = page(
            "<p>Done <img class=\"emoticon\" data-name=\"tick\" src=\"/images/icons/tick.svg\"> \
             and <img class=\"emoticon\" data-name=\"odd\" alt=\"(odd)\" src=\"x.png\"></p>",
        );
        assert_eq!(
            blocks,
            vec![para(vec![Inline::text("Done \u{2705} and (odd)")])]
        );
        assert_eq!(notes.count(NoteCode::EmojiAsText), 2);
    }

    #[test]
    fn tasks_are_boxed_bullets_counted_once_per_list() {
        let (blocks, notes) = page(
            "<ul><li class=\"task\"><input type=\"checkbox\"> Open</li>\
             <li class=\"task checked\"><input type=\"checkbox\" checked> Done</li>\
             <li>Plain</li></ul>",
        );
        assert_eq!(
            blocks,
            vec![Block::BulletList(vec![
                ListItem {
                    blocks: vec![para(vec![Inline::text("\u{2610} Open")])]
                },
                ListItem {
                    blocks: vec![para(vec![Inline::text("\u{2611} Done")])]
                },
                ListItem {
                    blocks: vec![para(vec![Inline::text("Plain")])]
                },
            ])]
        );
        assert_eq!(notes.count(NoteCode::ChecklistsAsBullets), 1);
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 0);
    }

    #[test]
    fn diagrams_are_their_image_or_their_name() {
        let (blocks, notes) = page(
            "<div class=\"drawio\"><img src=\"files/net.png\" alt=\"Network\"></div>\
             <p><span class=\"gliffy\" data-name=\"Rack layout\"></span></p>",
        );
        assert_eq!(
            blocks,
            vec![
                para(vec![Inline::Image(Image {
                    src: "files/net.png".into(),
                    alt: Some("Network".into()),
                    ..Image::default()
                })]),
                para(vec![Inline::text("Rack layout")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::DiagramsAsImages), 1);
        assert_eq!(notes.count(NoteCode::DiagramsLeftOut), 1);
    }

    #[test]
    fn includes_link_and_count_when_their_target_resolves() {
        let (blocks, notes) = page(
            "<p><span class=\"include\" data-href=\"https://wiki.example.com/x\">Setup</span></p>\
             <p><span class=\"include\" data-href=\"missing.html\">Gone</span></p>",
        );
        assert_eq!(
            blocks,
            vec![
                para(vec![linked(
                    "Included from Setup",
                    "https://wiki.example.com/x"
                )]),
                // The normaliser drops the destination the resolver refuses.
                para(vec![linked("Included from Gone", "missing.html")]),
            ]
        );
        assert_eq!(notes.count(NoteCode::IncludesAsLinks), 1);
    }

    #[test]
    fn an_image_rewrite_takes_the_dialects_source() {
        let (blocks, _) = page(
            "<p><img class=\"attached\" src=\"thumbnails/1/a.png\" data-original=\"attachments/1/a.png\" \
             data-alias=\"Rack\" width=\"300\"></p>",
        );
        assert_eq!(
            blocks,
            vec![para(vec![Inline::Image(Image {
                src: "attachments/1/a.png".into(),
                alt: Some("Rack".into()),
                width: Some(300),
                ..Image::default()
            })])]
        );
    }

    #[test]
    fn a_dialect_cannot_bring_back_a_script() {
        struct Unwrapping;
        impl Dialect for Unwrapping {
            fn classify(&self, _el: &ElementView) -> Option<Rewrite> {
                Some(Rewrite::Unwrap(None))
            }
        }
        let mut notes = Notes::default();
        let document = read_document(
            "<p>Hi<script>alert(1)</script><svg><text>x</text></svg></p>",
            &[],
            &Unwrapping,
            &DefaultResolver,
            &mut notes,
        );
        assert_eq!(document.blocks, vec![para(vec![Inline::text("Hi")])]);
        assert_eq!(notes.count(NoteCode::HtmlRemoved), 2);
    }

    #[test]
    fn markdowns_html_has_no_dialect() {
        let (blocks, notes) = read("<details><summary>More</summary><p>x</p></details>");
        assert_eq!(
            blocks,
            vec![para(vec![bold("More")]), para(vec![Inline::text("x")])]
        );
        assert_eq!(notes.count(NoteCode::ExpandsShownOpen), 0);
    }

    const PAGE: &str = "<!DOCTYPE html><html><head><title>IT : VPN</title>\
        <script>bad()</script></head><body>\
        <div id=\"breadcrumbs\">Home / IT</div>\
        <div class=\"wiki\"><div id=\"main-content\"><p>Main</p></div></div>\
        <div id=\"content\"><p>Content</p></div>\
        <div id=\"footer\">Powered by</div></body></html>";

    fn content_text(raw: &str, content: &[Selector]) -> Vec<Block> {
        let mut notes = Notes::default();
        read_document(raw, content, &DefaultDialect, &DefaultResolver, &mut notes).blocks
    }

    #[test]
    fn the_content_root_is_the_first_selector_that_matches() {
        let main = vec![para(vec![Inline::text("Main")])];
        let selectors = [Selector::Id("main-content"), Selector::Id("content")];
        assert_eq!(content_text(PAGE, &selectors), main);
        // Earlier selectors win over earlier elements.
        assert_eq!(
            content_text(
                PAGE,
                &[Selector::Id("content"), Selector::Id("main-content")]
            ),
            vec![para(vec![Inline::text("Content")])]
        );
        // A missing selector falls through to the next.
        let no_main = PAGE.replace("main-content", "other");
        assert_eq!(
            content_text(&no_main, &selectors),
            vec![para(vec![Inline::text("Content")])]
        );
        assert_eq!(content_text(PAGE, &[Selector::Class("wiki")]), main);
        assert_eq!(content_text(PAGE, &[Selector::Tag("p")]), main);
    }

    #[test]
    fn with_no_match_the_body_is_read_and_the_head_never() {
        let blocks = content_text(PAGE, &[Selector::Id("nowhere")]);
        assert_eq!(
            blocks,
            vec![
                para(vec![Inline::text("Home / IT")]),
                para(vec![Inline::text("Main")]),
                para(vec![Inline::text("Content")]),
                para(vec![Inline::text("Powered by")]),
            ]
        );
        // A fragment parses as a page's body.
        assert_eq!(
            content_text("<p>One</p>two", &[]),
            vec![
                para(vec![Inline::text("One")]),
                para(vec![Inline::text("two")])
            ]
        );
    }

    #[test]
    fn a_very_deep_page_is_read_as_flat_text() {
        let raw = format!(
            "<html><body>{}deep{}</body></html>",
            "<div>".repeat(5_000),
            "</div>".repeat(5_000)
        );
        let mut notes = Notes::default();
        let document = read_document(&raw, &[], &TestDialect, &DefaultResolver, &mut notes);
        assert_eq!(document.blocks, vec![para(vec![Inline::text("deep")])]);
        assert_eq!(notes.count(NoteCode::HtmlAsText), 1);
    }

    #[test]
    fn an_element_view_reads_its_neighbours() {
        let nodes =
            fragment("<div id=\"a\" class=\"x y\"><p>one <b>two</b></p><span>three</span></div>");
        let div = ElementView::of(&nodes[0]).unwrap();
        assert_eq!(div.tag(), "div");
        assert_eq!(div.id().as_deref(), Some("a"));
        assert!(div.has_class("y") && !div.has_class("x y"));
        assert_eq!(div.text(), "one twothree");
        let children: Vec<String> = div.children().iter().map(|c| c.tag().to_string()).collect();
        assert_eq!(children, vec!["p", "span"]);
        let b = div.find(&Selector::Tag("b")).unwrap();
        assert_eq!(b.text(), "two");
        assert_eq!(b.parent().unwrap().tag(), "p");
        assert_eq!(b.closest(&Selector::Id("a")).unwrap().tag(), "div");
        assert!(b.closest(&Selector::Class("nowhere")).is_none());
    }

    #[test]
    fn single_tags_decode_their_attributes() {
        let tag = read_tag("<span data-name=\"A &amp; B &quot;C&quot;\">").unwrap();
        assert_eq!(tag.name, "span");
        assert_eq!(tag.attr("data-name"), Some("A & B \"C\""));
        assert!(read_tag("</a>").unwrap().closing);
    }
}
