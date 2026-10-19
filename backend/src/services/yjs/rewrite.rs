//! Rewrite the references a stored document holds: the ticket a
//! `ticket_link` names, the person a `mention` names, the ticket folder a
//! notes `image` loads from, and the page an `embedded_document` shows.
//!
//! [`rewrite`] edits the document in place, as one more Yjs change on top
//! of its history, and writes the full state back. Everything it doesn't
//! rewrite (marks, attributes it doesn't read, deleted content) stays as it
//! was. A reference [`Targets`] can't place becomes what the Markdown
//! converter makes of one it can't keep (`docs_markdown::normalise`): a
//! mention its "@name", a ticket link its link text, an image its alt text,
//! an embedded page a paragraph with its title. Each lands where the editor
//! schema allows it (text in the inline content it came from, a paragraph
//! where a block was), and a run of text stays one XmlText, so y-prosemirror
//! builds the result without deleting anything.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use yrs::types::text::YChange;
use yrs::types::Attrs;
use yrs::{
    Any, Doc, Out, ReadTxn, Text, Transact, TransactionMut, Xml, XmlElementPrelim, XmlElementRef,
    XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
};

use super::{attr_string, load_with_client_id, FRAGMENT};

/// Where each reference points after the change. `None`: the target doesn't
/// have it.
pub trait Targets {
    /// The id of the ticket the id `id` names.
    fn ticket(&self, id: &str) -> Option<String>;
    /// The uuid of the person the uuid `uuid` names.
    fn user(&self, uuid: &str) -> Option<String>;
    /// The uuid of the documentation page the uuid `uuid` names.
    fn page(&self, uuid: &str) -> Option<String>;
}

/// What a rewrite changed, by node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub ticket_links: usize,
    pub mentions: usize,
    pub images: usize,
    pub embeds: usize,
    /// Ticket links to a ticket the target doesn't have, now their text.
    pub ticket_links_as_text: usize,
    /// Mentions of someone the target doesn't have, now "@name".
    pub mentions_as_text: usize,
    /// Notes images from a ticket the target doesn't have, now their alt
    /// text.
    pub images_as_text: usize,
    /// Embedded pages the target doesn't have, now their title.
    pub embeds_as_text: usize,
}

impl Counts {
    /// References the target couldn't place.
    pub fn unresolved(&self) -> usize {
        self.ticket_links_as_text
            + self.mentions_as_text
            + self.images_as_text
            + self.embeds_as_text
    }

    fn changed(&self) -> bool {
        self.unresolved() + self.ticket_links + self.mentions + self.images + self.embeds > 0
    }

    pub fn add(&mut self, other: &Counts) {
        self.ticket_links += other.ticket_links;
        self.mentions += other.mentions;
        self.images += other.images;
        self.embeds += other.embeds;
        self.ticket_links_as_text += other.ticket_links_as_text;
        self.mentions_as_text += other.mentions_as_text;
        self.images_as_text += other.images_as_text;
        self.embeds_as_text += other.embeds_as_text;
    }
}

/// The result of [`rewrite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rewrite {
    /// Nothing in the document names anything that moves.
    Unchanged,
    /// The document's new full state and state vector.
    Rewritten {
        update: Vec<u8>,
        state_vector: Vec<u8>,
        counts: Counts,
    },
    /// The bytes aren't a Yjs v1 update.
    NotADocument,
}

/// A notes image's ticket: `…/tickets/{id}/notes/…`, under `/uploads/` or
/// `/api/files/`, relative or absolute.
static NOTES_PATH: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"(/tickets/)(\d{1,10})(/notes/)").expect("notes path pattern")
});

/// The ticket id in a notes image's `src`, if it is one.
pub fn notes_image_ticket(src: &str) -> Option<&str> {
    NOTES_PATH
        .captures(src)
        .and_then(|c| c.get(2))
        .map(|m| m.as_str())
}

/// `src` with its notes folder's ticket id replaced by `ticket`.
pub fn with_notes_image_ticket(src: &str, ticket: &str) -> String {
    NOTES_PATH
        .replacen(src, 1, |c: &regex::Captures| {
            format!("{}{ticket}{}", &c[1], &c[3])
        })
        .into_owned()
}

/// Rewrite `update`'s references through `targets`.
pub fn rewrite(update: &[u8], targets: &dyn Targets) -> Rewrite {
    rewrite_with_client_id(update, targets, None)
}

/// [`rewrite`], writing the change under `client_id` when given (tests).
pub fn rewrite_with_client_id(
    update: &[u8],
    targets: &dyn Targets,
    client_id: Option<u64>,
) -> Rewrite {
    let Some(doc) = load_with_client_id(update, client_id) else {
        return Rewrite::NotADocument;
    };
    let (changes, counts) = {
        let txn = doc.transact();
        let mut changes = Vec::new();
        let mut counts = Counts::default();
        if let Some(fragment) = txn.get_xml_fragment(FRAGMENT) {
            for child in fragment.children(&txn) {
                collect(&txn, child, targets, &mut changes, &mut counts);
            }
        }
        (changes, counts)
    };
    if !counts.changed() {
        return Rewrite::Unchanged;
    }
    apply(&doc, changes);
    let (update, state_vector) = super::encode(&doc);
    Rewrite::Rewritten {
        update,
        state_vector,
        counts,
    }
}

/// One edit, decided while reading and made after.
enum Change {
    Set(XmlElementRef, &'static str, Any),
    Remove(XmlElementRef, &'static str),
    /// An inline node becomes this text in its place.
    InlineText(XmlElementRef, String),
    /// A block becomes a paragraph holding this text.
    Paragraph(XmlElementRef, String),
}

/// `new` with the JSON type `old` had: a number stays a number.
fn retyped(old: Option<Out>, new: &str) -> Any {
    match old {
        Some(Out::Any(Any::Number(_))) => new
            .parse::<f64>()
            .map(Any::Number)
            .unwrap_or_else(|_| Any::from(new)),
        _ => Any::from(new),
    }
}

fn collect<T: yrs::ReadTxn>(
    txn: &T,
    node: XmlOut,
    targets: &dyn Targets,
    changes: &mut Vec<Change>,
    counts: &mut Counts,
) {
    let XmlOut::Element(el) = node else {
        return;
    };
    let attr = |name: &str| attr_string(&el, txn, name).filter(|v| !v.trim().is_empty());
    match el.tag().as_ref() {
        "ticket_link" => {
            if let Some(old) = attr("ticketId") {
                match targets.ticket(old.trim()) {
                    Some(new) if new == old.trim() => {}
                    Some(new) => {
                        let href = match new.parse::<i32>() {
                            Ok(id) => crate::utils::ticket_link::ticket_route_by_id(id),
                            Err(_) => attr("href").unwrap_or_default(),
                        };
                        changes.push(Change::Set(
                            el.clone(),
                            "ticketId",
                            retyped(el.get_attribute(txn, "ticketId"), &new),
                        ));
                        changes.push(Change::Set(el.clone(), "href", Any::from(href)));
                        counts.ticket_links += 1;
                    }
                    None => {
                        let label = attr("href").unwrap_or(old);
                        changes.push(Change::InlineText(el.clone(), label));
                        counts.ticket_links_as_text += 1;
                    }
                }
            }
        }
        "mention" => {
            if let Some(old) = attr("uuid") {
                match targets.user(old.trim()) {
                    Some(new) if new.eq_ignore_ascii_case(old.trim()) => {}
                    Some(new) => {
                        changes.push(Change::Set(el.clone(), "uuid", Any::from(new)));
                        // The avatar path named the archive account.
                        if el.get_attribute(txn, "avatarUrl").is_some() {
                            changes.push(Change::Remove(el.clone(), "avatarUrl"));
                        }
                        counts.mentions += 1;
                    }
                    None => {
                        let text = attr("name").map(|n| format!("@{n}")).unwrap_or_default();
                        changes.push(Change::InlineText(el.clone(), text));
                        counts.mentions_as_text += 1;
                    }
                }
            }
        }
        "image" => {
            if let Some(src) = attr("src") {
                if let Some(old) = notes_image_ticket(&src) {
                    match targets.ticket(old) {
                        Some(new) if new == old => {}
                        Some(new) => {
                            let src = with_notes_image_ticket(&src, &new);
                            changes.push(Change::Set(el.clone(), "src", Any::from(src)));
                            counts.images += 1;
                        }
                        None => {
                            let alt = attr("alt").unwrap_or_default();
                            changes.push(Change::InlineText(el.clone(), alt));
                            counts.images_as_text += 1;
                        }
                    }
                }
            }
        }
        "embedded_document" => {
            if let Some(old) = attr("documentUuid") {
                match targets.page(old.trim()) {
                    Some(new) if new.eq_ignore_ascii_case(old.trim()) => {}
                    Some(new) => {
                        changes.push(Change::Set(el.clone(), "documentUuid", Any::from(new)));
                        counts.embeds += 1;
                    }
                    None => {
                        let title = attr("documentTitle").unwrap_or_else(|| "Untitled".into());
                        changes.push(Change::Paragraph(el.clone(), title.trim().to_string()));
                        counts.embeds_as_text += 1;
                    }
                }
            }
        }
        _ => {}
    }
    for child in el.children(txn) {
        collect(txn, child, targets, changes, counts);
    }
}

/// An element's parent, which may be the document's fragment.
enum Parent {
    Element(XmlElementRef),
    Fragment(yrs::XmlFragmentRef),
}

impl Parent {
    fn of(el: &XmlElementRef) -> Option<Parent> {
        match el.parent()? {
            XmlOut::Element(p) => Some(Parent::Element(p)),
            XmlOut::Fragment(f) => Some(Parent::Fragment(f)),
            XmlOut::Text(_) => None,
        }
    }

    fn get(&self, txn: &TransactionMut, index: u32) -> Option<XmlOut> {
        match self {
            Parent::Element(p) => p.get(txn, index),
            Parent::Fragment(f) => f.get(txn, index),
        }
    }

    fn index_of(&self, txn: &TransactionMut, el: &XmlElementRef) -> Option<u32> {
        let children: Vec<XmlOut> = match self {
            Parent::Element(p) => p.children(txn).collect(),
            Parent::Fragment(f) => f.children(txn).collect(),
        };
        children
            .iter()
            .position(|c| matches!(c, XmlOut::Element(e) if e == el))
            .and_then(|i| u32::try_from(i).ok())
    }

    fn remove(&self, txn: &mut TransactionMut, index: u32) {
        match self {
            Parent::Element(p) => p.remove_range(txn, index, 1),
            Parent::Fragment(f) => f.remove_range(txn, index, 1),
        }
    }

    fn insert_text(&self, txn: &mut TransactionMut, index: u32, text: &str) {
        match self {
            Parent::Element(p) => {
                p.insert(txn, index, XmlTextPrelim::new(text));
            }
            Parent::Fragment(f) => {
                f.insert(txn, index, XmlTextPrelim::new(text));
            }
        }
    }

    fn insert_paragraph(&self, txn: &mut TransactionMut, index: u32) -> XmlElementRef {
        match self {
            Parent::Element(p) => p.insert(txn, index, XmlElementPrelim::empty("paragraph")),
            Parent::Fragment(f) => f.insert(txn, index, XmlElementPrelim::empty("paragraph")),
        }
    }
}

fn text_at(parent: &Parent, txn: &TransactionMut, index: u32) -> Option<XmlTextRef> {
    match parent.get(txn, index)? {
        XmlOut::Text(t) => Some(t),
        _ => None,
    }
}

fn apply(doc: &Doc, changes: Vec<Change>) {
    let mut txn = doc.transact_mut();
    for change in changes {
        match change {
            Change::Set(el, name, value) => {
                el.insert_attribute(&mut txn, name, value);
            }
            Change::Remove(el, name) => el.remove_attribute(&mut txn, &name),
            Change::InlineText(el, text) => inline_to_text(&mut txn, &el, &text),
            Change::Paragraph(el, text) => {
                let Some(parent) = Parent::of(&el) else {
                    continue;
                };
                let Some(i) = parent.index_of(&txn, &el) else {
                    continue;
                };
                parent.remove(&mut txn, i);
                let paragraph = parent.insert_paragraph(&mut txn, i);
                if !text.is_empty() {
                    paragraph.push_back(&mut txn, XmlTextPrelim::new(text));
                }
            }
        }
    }
}

/// Replace the inline node `el` with `text`, unformatted, joining the text
/// runs on either side into one.
fn inline_to_text(txn: &mut TransactionMut, el: &XmlElementRef, text: &str) {
    let Some(parent) = Parent::of(el) else {
        return;
    };
    let Some(i) = parent.index_of(txn, el) else {
        return;
    };
    let before = i.checked_sub(1).and_then(|b| text_at(&parent, txn, b));
    let after = text_at(&parent, txn, i + 1);
    parent.remove(txn, i);
    match (before, after) {
        (Some(before), after) => {
            let end = before.len(txn);
            before.insert_with_attributes(txn, end, text, Attrs::new());
            if let Some(after) = after {
                for chunk in after.diff(txn, YChange::identity) {
                    let attrs = chunk.attributes.map(|a| *a).unwrap_or_default();
                    let end = before.len(txn);
                    match chunk.insert {
                        Out::Any(Any::String(s)) => {
                            before.insert_with_attributes(txn, end, &s, attrs);
                        }
                        Out::Any(other) => {
                            before.insert_embed_with_attributes(txn, end, other, attrs);
                        }
                        _ => {}
                    }
                }
                // `after` moved up one when `el` went.
                parent.remove(txn, i);
            }
        }
        (None, Some(after)) => after.insert_with_attributes(txn, 0, text, Attrs::new()),
        (None, None) => {
            if !text.is_empty() {
                parent.insert_text(txn, i, text);
            }
        }
    }
}

#[cfg(test)]
mod tests;
