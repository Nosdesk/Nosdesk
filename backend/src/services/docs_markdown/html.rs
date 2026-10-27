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
    Alignment, Block, Image, ImageAlign, Inline, Link, ListItem, Marks, Table, TableCell,
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

/// Read one HTML block.
pub fn read_block(raw: &str, resolver: &dyn Resolver, notes: &mut Notes) -> ReadBlock {
    let tags = tags(raw);
    if nesting(&tags) > MAX_NESTING {
        // Too deep to walk (or to drop) as a tree: its text, flat.
        notes.add(NoteCode::HtmlAsText);
        let text = flat_text(raw);
        let blocks = text
            .split("\n\n")
            .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|p| !p.is_empty())
            .map(|p| Block::Paragraph(vec![Inline::text(p)]))
            .collect();
        return ReadBlock {
            blocks,
            left_open: None,
        };
    }
    let mut reader = Reader {
        resolver,
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
        for node in nodes {
            match &node.data {
                Data::Text(text) => run.text(&text.borrow(), &Marks::default()),
                Data::Comment => self.notes.add(NoteCode::HtmlRemoved),
                Data::Element { .. } => {
                    let name = tag_name(node).unwrap_or("");
                    if is_removed_with_content(name) || name.is_empty() {
                        self.notes.add(NoteCode::HtmlRemoved);
                    } else if is_block_element(name) {
                        run.flush_into(out);
                        self.block(node, name, out);
                    } else {
                        self.inline(node, &mut run, &Marks::default());
                    }
                }
                _ => {}
            }
        }
        run.flush_into(out);
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
                items.push(ListItem {
                    blocks: self.children_blocks(child),
                });
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
                self.inline_element(node, name, run, marks);
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

    #[test]
    fn single_tags_decode_their_attributes() {
        let tag = read_tag("<span data-name=\"A &amp; B &quot;C&quot;\">").unwrap();
        assert_eq!(tag.name, "span");
        assert_eq!(tag.attr("data-name"), Some("A & B \"C\""));
        assert!(read_tag("</a>").unwrap().closing);
    }
}
