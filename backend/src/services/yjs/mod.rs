//! Nosdesk documents as Yjs: the pieces every reader and writer of a stored
//! document shares.
//!
//! A document (a ticket's notes, a documentation page, a collection's
//! description, and each of their revisions) is a Yjs v1 update holding one
//! `prosemirror` XmlFragment, laid out the way y-prosemirror lays it out.
//! The Markdown converter writes them (`services::docs_markdown`), the
//! Markdown export reads them (`utils::markdown_export`), and a workspace
//! import rewrites the references inside them ([`rewrite`]).

pub mod rewrite;

use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, ClientID, Doc, Options, Out, ReadTxn, StateVector, Transact, Update, WriteTxn, Xml,
    XmlElementRef,
};

/// The XmlFragment y-prosemirror binds the editor to.
pub const FRAGMENT: &str = "prosemirror";

/// A stored document, `None` when the bytes aren't a v1 update. Deleted
/// content is kept (no garbage collection), so writing it back loses
/// nothing.
pub fn load(yjs_document: &[u8]) -> Option<Doc> {
    load_with_client_id(yjs_document, None)
}

/// [`load`], writing any change under `client_id` when given. Tests fix it
/// for reproducible output; real rewrites take a random one, as two writers
/// sharing an id corrupt each other.
pub fn load_with_client_id(yjs_document: &[u8], client_id: Option<u64>) -> Option<Doc> {
    let mut options = Options {
        skip_gc: true,
        ..Default::default()
    };
    if let Some(id) = client_id {
        options.client_id = ClientID::new(id);
    }
    let doc = Doc::with_options(options);
    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment(FRAGMENT);
    }
    let update = Update::decode_v1(yjs_document).ok()?;
    doc.transact_mut().apply_update(update).ok()?;
    Some(doc)
}

/// A document's full state as a v1 update, and its state vector: the two
/// columns a stored document keeps.
pub fn encode(doc: &Doc) -> (Vec<u8>, Vec<u8>) {
    let txn = doc.transact();
    (
        txn.encode_state_as_update_v1(&StateVector::default()),
        txn.state_vector().encode_v1(),
    )
}

/// An attribute value as text: strings as they are, whole numbers without a
/// fraction.
pub fn any_string(value: &Any) -> Option<String> {
    match value {
        Any::Null | Any::Undefined => None,
        Any::String(s) => Some(s.to_string()),
        Any::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            Some(format!("{}", *n as i64))
        }
        Any::Number(n) => Some(n.to_string()),
        Any::BigInt(n) => Some(n.to_string()),
        Any::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// An element's attribute as text ([`any_string`]).
pub fn attr_string<T: ReadTxn>(elem: &XmlElementRef, txn: &T, name: &str) -> Option<String> {
    match elem.get_attribute(txn, name)? {
        Out::Any(value) => any_string(&value),
        other => Some(other.to_string(txn)),
    }
}
