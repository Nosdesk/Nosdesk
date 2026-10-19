//! Writes a normalised [`ir`](super::ir) tree to Yjs the way y-prosemirror
//! 1.3.7 does (`createTypeFromElementNode` and `createTypeFromTextNodes` in
//! `sync-plugin.js`):
//!
//! - The document is the `prosemirror` XmlFragment; each node is an
//!   XmlElement named after its node type.
//! - Every attr that is not null is set, defaults included (`level`,
//!   `order`, `documentTitle`), with its JSON type: numbers as numbers.
//!   Null attrs and `ychange` are never written.
//! - Each run of adjacent text nodes is one XmlText, one delta insert per
//!   text node. Marks are formatting attributes named after the mark, valued
//!   with the mark's attrs: `{}` for `em`, `strong` and `code`,
//!   `{href, title}` for `link`. An empty node has no XmlText at all.
//!
//! The encoder does no validation; [`normalise`](super::normalise) must run
//! first.

use std::collections::HashMap;
use std::sync::Arc;

use yrs::types::xml::{XmlDeltaPrelim, XmlIn};
use yrs::types::Delta;
use yrs::{
    Any, Doc, ReadTxn, StateVector, Transact, TransactionMut, WriteTxn, Xml, XmlElementPrelim,
    XmlElementRef, XmlFragment,
};

use super::ir::{Block, Document, Inline, ListItem, Marks, Text};
use super::normalise::table_to_code_block;
use super::Notes;

/// The XmlFragment y-prosemirror binds the editor to.
pub const FRAGMENT: &str = "prosemirror";

/// Encode a document as a Yjs v1 update.
pub fn encode(document: &Document) -> Vec<u8> {
    encode_into(Doc::new(), document)
}

/// Encode with a fixed client id, so fixtures are reproducible. Real
/// documents use a random id: two writers sharing one corrupt each other.
pub fn encode_with_client_id(document: &Document, client_id: u64) -> Vec<u8> {
    encode_into(Doc::with_client_id(client_id), document)
}

fn encode_into(doc: Doc, document: &Document) -> Vec<u8> {
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
        for block in &document.blocks {
            write_block(&mut txn, &fragment, block);
        }
    }
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&StateVector::default())
}

fn element<P: XmlFragment>(txn: &mut TransactionMut, parent: &P, name: &str) -> XmlElementRef {
    parent.push_back(txn, XmlElementPrelim::empty(name))
}

fn number(n: u32) -> Any {
    Any::Number(f64::from(n))
}

fn write_block<P: XmlFragment>(txn: &mut TransactionMut, parent: &P, block: &Block) {
    match block {
        Block::Paragraph(inlines) => {
            let el = element(txn, parent, "paragraph");
            write_inlines(txn, &el, inlines);
        }
        Block::Heading { level, content } => {
            let el = element(txn, parent, "heading");
            el.insert_attribute(txn, "level", number(u32::from(*level)));
            write_inlines(txn, &el, content);
        }
        Block::Blockquote(blocks) => {
            let el = element(txn, parent, "blockquote");
            for child in blocks {
                write_block(txn, &el, child);
            }
        }
        Block::CodeBlock { language, text } => {
            let el = element(txn, parent, "code_block");
            if let Some(language) = language {
                el.insert_attribute(txn, "language", Any::from(language.as_str()));
            }
            if !text.is_empty() {
                el.push_back(
                    txn,
                    XmlIn::from(XmlDeltaPrelim {
                        attributes: HashMap::new(),
                        delta: vec![Delta::insert(text.as_str())],
                    }),
                );
            }
        }
        Block::HorizontalRule => {
            element(txn, parent, "horizontal_rule");
        }
        Block::BulletList(items) => {
            let el = element(txn, parent, "bullet_list");
            write_items(txn, &el, items);
        }
        Block::OrderedList { order, items } => {
            let el = element(txn, parent, "ordered_list");
            el.insert_attribute(txn, "order", number(*order));
            write_items(txn, &el, items);
        }
        Block::EmbeddedDocument {
            document_uuid,
            document_title,
        } => {
            let el = element(txn, parent, "embedded_document");
            el.insert_attribute(txn, "documentUuid", Any::from(document_uuid.as_str()));
            el.insert_attribute(txn, "documentTitle", Any::from(document_title.as_str()));
        }
        // The normaliser lowers tables before encoding; this keeps a
        // document that skipped it from carrying a node no client can build.
        Block::Table(table) => {
            let lowered = table_to_code_block(table.clone(), &mut Notes::default());
            write_block(txn, parent, &lowered);
        }
    }
}

fn write_items(txn: &mut TransactionMut, list: &XmlElementRef, items: &[ListItem]) {
    for item in items {
        let li = element(txn, list, "list_item");
        for block in &item.blocks {
            write_block(txn, &li, block);
        }
    }
}

/// Inline content: one XmlText per run of text, an element per other node.
fn write_inlines(txn: &mut TransactionMut, parent: &XmlElementRef, inlines: &[Inline]) {
    let mut run: Vec<&Text> = Vec::new();
    for inline in inlines {
        if let Inline::Text(text) = inline {
            run.push(text);
            continue;
        }
        flush_text(txn, parent, &mut run);
        match inline {
            Inline::Text(_) => {}
            Inline::Image(image) => {
                let el = element(txn, parent, "image");
                el.insert_attribute(txn, "src", Any::from(image.src.as_str()));
                if let Some(alt) = &image.alt {
                    el.insert_attribute(txn, "alt", Any::from(alt.as_str()));
                }
                if let Some(title) = &image.title {
                    el.insert_attribute(txn, "title", Any::from(title.as_str()));
                }
                if let Some(width) = image.width {
                    el.insert_attribute(txn, "width", number(width));
                }
                if let Some(align) = image.align {
                    el.insert_attribute(txn, "align", Any::from(align.as_str()));
                }
            }
            Inline::HardBreak => {
                element(txn, parent, "hard_break");
            }
            Inline::TicketLink { ticket_id, href } => {
                let el = element(txn, parent, "ticket_link");
                el.insert_attribute(txn, "ticketId", Any::from(ticket_id.as_str()));
                el.insert_attribute(txn, "href", Any::from(href.as_str()));
            }
            Inline::Mention {
                uuid,
                name,
                avatar_url,
            } => {
                let el = element(txn, parent, "mention");
                el.insert_attribute(txn, "uuid", Any::from(uuid.as_str()));
                el.insert_attribute(txn, "name", Any::from(name.as_str()));
                if let Some(url) = avatar_url {
                    el.insert_attribute(txn, "avatarUrl", Any::from(url.as_str()));
                }
            }
        }
    }
    flush_text(txn, parent, &mut run);
}

fn flush_text(txn: &mut TransactionMut, parent: &XmlElementRef, run: &mut Vec<&Text>) {
    if run.is_empty() {
        return;
    }
    let delta = run
        .drain(..)
        .map(|text| {
            let attrs = mark_attributes(&text.marks);
            if attrs.is_empty() {
                Delta::insert(text.text.as_str())
            } else {
                Delta::insert_with(text.text.as_str(), attrs)
            }
        })
        .collect();
    parent.push_back(
        txn,
        XmlIn::from(XmlDeltaPrelim {
            attributes: HashMap::new(),
            delta,
        }),
    );
}

/// The formatting attributes y-prosemirror writes for a text node's marks.
fn mark_attributes(marks: &Marks) -> HashMap<Arc<str>, Any> {
    let empty = || Any::from(HashMap::<String, Any>::new());
    let mut attrs = HashMap::new();
    if let Some(link) = &marks.link {
        let title = link.title.as_deref().map(Any::from).unwrap_or(Any::Null);
        attrs.insert(
            Arc::from("link"),
            Any::from(HashMap::from([
                ("href".to_string(), Any::from(link.href.as_str())),
                ("title".to_string(), title),
            ])),
        );
    }
    if marks.em {
        attrs.insert(Arc::from("em"), empty());
    }
    if marks.strong {
        attrs.insert(Arc::from("strong"), empty());
    }
    if marks.code {
        attrs.insert(Arc::from("code"), empty());
    }
    attrs
}

/// Reads a y-prosemirror document back as ProseMirror JSON, the way
/// `yXmlFragmentToProsemirrorJSON` does. For tests and fixture checks.
#[cfg(test)]
pub(crate) mod decode {
    use serde_json::{json, Map, Value};
    use yrs::types::text::YChange;
    use yrs::types::Attrs;
    use yrs::updates::decoder::Decode;
    use yrs::{Any, Doc, Out, Text, Transact, Update, WriteTxn, Xml, XmlFragment, XmlOut};

    pub fn to_json(update: &[u8]) -> Value {
        let doc = Doc::new();
        let mut txn = doc.transact_mut();
        let decoded = Update::decode_v1(update).expect("valid update");
        txn.apply_update(decoded).expect("update applies");
        let fragment = txn.get_or_insert_xml_fragment(super::FRAGMENT);
        let content: Vec<Value> = fragment
            .children(&txn)
            .flat_map(|child| node(&txn, child))
            .collect();
        json!({ "type": "doc", "content": content })
    }

    fn node<T: yrs::ReadTxn>(txn: &T, child: XmlOut) -> Vec<Value> {
        match child {
            XmlOut::Element(el) => {
                let mut obj = Map::new();
                obj.insert("type".into(), Value::String(el.tag().to_string()));
                let mut attrs = Map::new();
                for (key, value) in el.attributes(txn) {
                    attrs.insert(key.to_string(), out(&value));
                }
                if !attrs.is_empty() {
                    obj.insert("attrs".into(), Value::Object(attrs));
                }
                let content: Vec<Value> = el
                    .children(txn)
                    .flat_map(|child| node(txn, child))
                    .collect();
                if !content.is_empty() {
                    obj.insert("content".into(), Value::Array(content));
                }
                vec![Value::Object(obj)]
            }
            XmlOut::Text(text) => text
                .diff(txn, YChange::identity)
                .into_iter()
                .map(|d| {
                    let mut obj = Map::new();
                    obj.insert("type".into(), "text".into());
                    obj.insert("text".into(), out(&d.insert));
                    if let Some(marks) = d.attributes.as_deref().map(marks) {
                        obj.insert("marks".into(), marks);
                    }
                    Value::Object(obj)
                })
                .collect(),
            XmlOut::Fragment(_) => Vec::new(),
        }
    }

    fn marks(attrs: &Attrs) -> Value {
        let mut names: Vec<_> = attrs.keys().collect();
        names.sort();
        Value::Array(
            names
                .into_iter()
                .map(|name| json!({ "type": name.as_ref(), "attrs": any(&attrs[name]) }))
                .collect(),
        )
    }

    fn out(value: &Out) -> Value {
        match value {
            Out::Any(a) => any(a),
            other => Value::String(format!("{other:?}")),
        }
    }

    fn any(value: &Any) -> Value {
        match value {
            Any::Null | Any::Undefined => Value::Null,
            Any::Bool(b) => Value::Bool(*b),
            Any::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
                json!(*n as i64)
            }
            Any::Number(n) => json!(n),
            Any::BigInt(n) => json!({ "bigint": n }),
            Any::String(s) => Value::String(s.to_string()),
            Any::Buffer(_) => Value::String("<buffer>".into()),
            Any::Array(items) => Value::Array(items.iter().map(any).collect()),
            Any::Map(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), any(v))).collect()),
        }
    }
}
