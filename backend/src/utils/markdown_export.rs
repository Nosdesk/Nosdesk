use std::collections::{HashMap, HashSet};
use std::panic;
use unic_langid::LanguageIdentifier;
use uuid::Uuid;
use yrs::{
    updates::decoder::Decode, Doc, GetString, Options, ReadTxn, Transact, Update, WriteTxn, Xml,
    XmlFragment, XmlOut,
};

use crate::db::DbConnection;
use crate::repository;
use crate::repository::PageAudience;
use crate::utils::i18n;

const MAX_EMBED_DEPTH: usize = 10;

/// Convert a Yjs document binary blob to Markdown
pub fn yjs_to_markdown(yjs_document: &[u8]) -> Option<String> {
    let options = Options {
        skip_gc: true,
        ..Default::default()
    };
    let doc = Doc::with_options(options);

    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment("prosemirror");
    }

    let update = match Update::decode_v1(yjs_document) {
        Ok(u) => u,
        Err(_) => return None,
    };

    {
        let mut txn = doc.transact_mut();
        if txn.apply_update(update).is_err() {
            return None;
        }
    }

    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror")?;

    // No connection to read numbers with: ticket links go by id.
    let numbers = &TicketNumbers::new();
    let mut output = String::new();
    for child in fragment.children(&txn) {
        let block = node_to_markdown(&child, &txn, 0, numbers);
        if !block.is_empty() {
            output.push_str(&block);
            output.push('\n');
        }
    }

    let trimmed = output.trim_end().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Everything needed to follow an `embedded_document` node: the connection to
/// read the embedded page with, and who the export is for.
///
/// They are bundled rather than passed separately because they must not be
/// separable. A page the caller may read can embed one they may not, so an
/// exporter holding a connection but no audience would silently render content
/// past the ACL. Requiring both to resolve an embed makes that a type error
/// rather than a discipline.
pub struct EmbedResolver<'a> {
    pub conn: &'a mut DbConnection,
    pub audience: PageAudience,
}

impl EmbedResolver<'_> {
    /// Reborrow for a recursive call. `Option<&mut _>` is not `Copy`, and
    /// `as_deref_mut` does not apply to a plain struct.
    fn reborrow(&mut self) -> EmbedResolver<'_> {
        EmbedResolver {
            conn: self.conn,
            audience: self.audience,
        }
    }
}

/// Convert a Yjs document to Markdown with recursive embed resolution
pub fn yjs_to_markdown_with_embeds(
    yjs_document: &[u8],
    resolver: &mut EmbedResolver<'_>,
    visited: &mut HashSet<Uuid>,
    current_uuid: Option<Uuid>,
    depth: usize,
    locale: &LanguageIdentifier,
) -> Option<String> {
    if depth > MAX_EMBED_DEPTH {
        return Some(format!(
            "> {}\n",
            i18n::tr(locale, "markdown-embed-depth-limit")
        ));
    }

    if let Some(uuid) = current_uuid {
        if visited.contains(&uuid) {
            return Some(format!(
                "> {}\n",
                i18n::tr(locale, "markdown-embed-circular")
            ));
        }
        visited.insert(uuid);
    }

    let options = Options {
        skip_gc: true,
        ..Default::default()
    };
    let doc = Doc::with_options(options);

    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment("prosemirror");
    }

    let update = match Update::decode_v1(yjs_document) {
        Ok(u) => u,
        Err(_) => return None,
    };

    {
        let mut txn = doc.transact_mut();
        if txn.apply_update(update).is_err() {
            return None;
        }
    }

    let txn = doc.transact();
    let fragment = txn.get_xml_fragment("prosemirror")?;

    let numbers = &linked_ticket_numbers(&fragment, &txn, &mut *resolver.conn);
    let mut output = String::new();
    for child in fragment.children(&txn) {
        let block = node_to_markdown_with_embeds(
            &child, &txn, 0, resolver, visited, depth, locale, numbers,
        );
        if !block.is_empty() {
            output.push_str(&block);
            output.push('\n');
        }
    }

    let trimmed = output.trim_end().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Ticket number by id, for the tickets a document links. A ticket missing
/// from it (deleted, or not in this workspace) is linked by id.
type TicketNumbers = HashMap<i32, i32>;

/// The numbers of the tickets the document links, read in the connection's
/// pinned workspace. Empty when it links none or the read fails.
fn linked_ticket_numbers(
    fragment: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
    conn: &mut DbConnection,
) -> TicketNumbers {
    fn collect(node: &XmlOut, txn: &yrs::Transaction, ids: &mut Vec<i32>) {
        match node {
            XmlOut::Element(elem) => {
                if elem.tag().as_ref() == "ticket_link" {
                    let id = elem
                        .get_attribute(txn, "ticketId")
                        .and_then(|v| v.to_string(txn).parse::<i32>().ok());
                    ids.extend(id);
                }
                for child in elem.children(txn) {
                    collect(&child, txn, ids);
                }
            }
            XmlOut::Fragment(frag) => {
                for child in frag.children(txn) {
                    collect(&child, txn, ids);
                }
            }
            XmlOut::Text(_) => {}
        }
    }
    let mut ids = Vec::new();
    for child in fragment.children(txn) {
        collect(&child, txn, &mut ids);
    }
    if ids.is_empty() {
        return TicketNumbers::new();
    }
    repository::tickets::numbers_in_pinned_workspace(conn, &ids).unwrap_or_default()
}

/// A `ticket_link` node as people quote it, `[Ticket #N](/tickets/N)`. A
/// ticket without a number here links through `/tickets/id/<id>`, which the
/// app redirects to the ticket's number.
fn ticket_link_markdown(
    elem: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    numbers: &TicketNumbers,
) -> String {
    let ticket_id = elem
        .get_attribute(txn, "ticketId")
        .map(|v| v.to_string(txn))
        .unwrap_or_default();
    let Ok(id) = ticket_id.parse::<i32>() else {
        return "Ticket".to_string();
    };
    match numbers.get(&id) {
        Some(&number) => format!(
            "[Ticket #{number}]({})",
            crate::utils::ticket_link::ticket_route(number)
        ),
        None => format!(
            "[Ticket]({})",
            crate::utils::ticket_link::ticket_route_by_id(id)
        ),
    }
}

/// Convert a single XML node to Markdown (block-level)
fn node_to_markdown(
    node: &XmlOut,
    txn: &yrs::Transaction,
    list_depth: usize,
    numbers: &TicketNumbers,
) -> String {
    match node {
        XmlOut::Text(text_ref) => get_text_safe(text_ref, txn),
        XmlOut::Element(elem) => {
            let tag = elem.tag().to_string();
            element_to_markdown(
                &tag,
                elem,
                txn,
                list_depth,
                None,
                &mut HashSet::new(),
                0,
                None,
                numbers,
            )
        }
        XmlOut::Fragment(frag) => {
            let mut out = String::new();
            for child in frag.children(txn) {
                out.push_str(&node_to_markdown(&child, txn, list_depth, numbers));
            }
            out
        }
    }
}

/// Convert a single XML node to Markdown with embed resolution
fn node_to_markdown_with_embeds(
    node: &XmlOut,
    txn: &yrs::Transaction,
    list_depth: usize,
    resolver: &mut EmbedResolver<'_>,
    visited: &mut HashSet<Uuid>,
    embed_depth: usize,
    locale: &LanguageIdentifier,
    numbers: &TicketNumbers,
) -> String {
    match node {
        XmlOut::Text(text_ref) => get_text_safe(text_ref, txn),
        XmlOut::Element(elem) => {
            let tag = elem.tag().to_string();
            element_to_markdown(
                &tag,
                elem,
                txn,
                list_depth,
                Some(resolver.reborrow()),
                visited,
                embed_depth,
                Some(locale),
                numbers,
            )
        }
        XmlOut::Fragment(frag) => {
            let mut out = String::new();
            for child in frag.children(txn) {
                out.push_str(&node_to_markdown_with_embeds(
                    &child,
                    txn,
                    list_depth,
                    resolver,
                    visited,
                    embed_depth,
                    locale,
                    numbers,
                ));
            }
            out
        }
    }
}

fn get_text_safe(text_ref: &yrs::XmlTextRef, txn: &yrs::Transaction) -> String {
    panic::catch_unwind(panic::AssertUnwindSafe(|| text_ref.get_string(txn))).unwrap_or_default()
}

/// Convert an XML element to Markdown based on its tag name
fn element_to_markdown(
    tag: &str,
    elem: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    list_depth: usize,
    mut resolver: Option<EmbedResolver<'_>>,
    visited: &mut HashSet<Uuid>,
    embed_depth: usize,
    locale: Option<&LanguageIdentifier>,
    numbers: &TicketNumbers,
) -> String {
    match tag {
        "paragraph" => {
            let text = collect_inline_children(elem, txn, numbers);
            format!("{}\n", text)
        }
        "heading" => {
            let level = elem
                .get_attribute(txn, "level")
                .and_then(|v| v.to_string(txn).parse::<usize>().ok())
                .unwrap_or(1)
                .min(6);
            let prefix = "#".repeat(level);
            let text = collect_inline_children(elem, txn, numbers);
            format!("{} {}\n", prefix, text)
        }
        "code_block" => {
            let language = elem
                .get_attribute(txn, "language")
                .map(|v| v.to_string(txn))
                .unwrap_or_default();
            let text = collect_raw_text(elem, txn);
            format!("```{}\n{}\n```\n", language, text)
        }
        "blockquote" => {
            let inner = collect_block_children(
                elem,
                txn,
                list_depth,
                resolver.as_mut().map(EmbedResolver::reborrow),
                visited,
                embed_depth,
                locale,
                numbers,
            );
            let quoted: String = inner
                .lines()
                .map(|line| format!("> {}", line))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n", quoted)
        }
        "bullet_list" => {
            let mut out = String::new();
            for child in elem.children(txn) {
                if let XmlOut::Element(li) = &child {
                    if li.tag().as_ref() == "list_item" {
                        let indent = "  ".repeat(list_depth);
                        let content = collect_list_item_content(
                            li,
                            txn,
                            list_depth,
                            resolver.as_mut().map(EmbedResolver::reborrow),
                            visited,
                            embed_depth,
                            locale,
                            numbers,
                        );
                        out.push_str(&format!("{}- {}\n", indent, content.trim_end()));
                    }
                }
            }
            out
        }
        "ordered_list" => {
            let mut out = String::new();
            let mut index = 1;
            for child in elem.children(txn) {
                if let XmlOut::Element(li) = &child {
                    if li.tag().as_ref() == "list_item" {
                        let indent = "  ".repeat(list_depth);
                        let content = collect_list_item_content(
                            li,
                            txn,
                            list_depth,
                            resolver.as_mut().map(EmbedResolver::reborrow),
                            visited,
                            embed_depth,
                            locale,
                            numbers,
                        );
                        out.push_str(&format!("{}{}. {}\n", indent, index, content.trim_end()));
                        index += 1;
                    }
                }
            }
            out
        }
        "horizontal_rule" => String::from("---\n"),
        "hard_break" => String::from("  \n"),
        "image" => {
            let src = elem
                .get_attribute(txn, "src")
                .map(|v| v.to_string(txn))
                .unwrap_or_default();
            let alt = elem
                .get_attribute(txn, "alt")
                .map(|v| v.to_string(txn))
                .unwrap_or_default();
            format!("![{}]({})\n", alt, src)
        }
        "ticket_link" => ticket_link_markdown(elem, txn, numbers),
        "mention" => {
            let name = elem
                .get_attribute(txn, "name")
                .map(|v| v.to_string(txn))
                .unwrap_or_else(|| "unknown".to_string());
            format!("@{}", name)
        }
        "embedded_document" => {
            let doc_uuid_str = elem
                .get_attribute(txn, "documentUuid")
                .map(|v| v.to_string(txn))
                .unwrap_or_default();
            let untitled_fallback = locale
                .map(|l| i18n::tr(l, "docs-untitled-page"))
                .unwrap_or_else(|| "Untitled".to_string());
            let doc_title = elem
                .get_attribute(txn, "documentTitle")
                .map(|v| v.to_string(txn))
                .unwrap_or(untitled_fallback);

            if let Some(mut resolver) = resolver {
                if let (Ok(uuid), Some(loc)) = (Uuid::parse_str(&doc_uuid_str), locale) {
                    if embed_depth < MAX_EMBED_DEPTH && !visited.contains(&uuid) {
                        let conn = &mut *resolver.conn;
                        // An embedded page carries its own ACL. Read it only if
                        // this export's audience may read it; otherwise fall
                        // through to the reference fallback below, which prints
                        // the title the embedding document already contains and
                        // none of the embedded content.
                        let readable = repository::get_documentation_page_by_uuid(&uuid, conn)
                            .ok()
                            .filter(|page| resolver.audience.can_read(resolver.conn, page.id));
                        if let Some(page) = readable {
                            let conn = &mut *resolver.conn;
                            let yjs_doc = page.yjs_document.as_ref()
                                .cloned()
                                .or_else(|| {
                                    repository::documentation_page_tickets::most_recent_resolves_ticket_id(conn, page.id)
                                        .ok()
                                        .flatten()
                                        .and_then(|tid| repository::get_article_content_by_ticket_id(conn, tid).ok())
                                        .and_then(|a| a.yjs_document)
                                });

                            if let Some(doc_bytes) = yjs_doc {
                                if let Some(content) = yjs_to_markdown_with_embeds(
                                    &doc_bytes,
                                    &mut resolver,
                                    visited,
                                    Some(uuid),
                                    embed_depth + 1,
                                    loc,
                                ) {
                                    let header = i18n::tr_with(
                                        loc,
                                        "markdown-embed-reference",
                                        &[("title", doc_title.clone().into())],
                                    );
                                    return format!(
                                        "\n---\n\n**{}**\n\n{}\n\n---\n",
                                        header, content
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Fallback: just show a reference (no embed-resolve possible
            // here, e.g. the no-locale path from yjs_to_markdown).
            let reference = locale
                .map(|l| {
                    i18n::tr_with(
                        l,
                        "markdown-embed-reference-fallback",
                        &[("title", doc_title.clone().into())],
                    )
                })
                .unwrap_or_else(|| format!("[Embedded: {}]", doc_title));
            format!("{}\n", reference)
        }
        _ => {
            // Unknown tag - collect inline children as text
            collect_inline_children(elem, txn, numbers)
        }
    }
}

/// Collect inline children of an element with markdown marks
fn collect_inline_children(
    elem: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    numbers: &TicketNumbers,
) -> String {
    let mut out = String::new();
    for child in elem.children(txn) {
        match &child {
            XmlOut::Text(text_ref) => {
                out.push_str(&get_text_safe(text_ref, txn));
            }
            XmlOut::Element(child_elem) => {
                let tag = child_elem.tag().to_string();
                match tag.as_str() {
                    "hard_break" | "br" => out.push_str("  \n"),
                    "image" => {
                        let src = child_elem
                            .get_attribute(txn, "src")
                            .map(|v| v.to_string(txn))
                            .unwrap_or_default();
                        let alt = child_elem
                            .get_attribute(txn, "alt")
                            .map(|v| v.to_string(txn))
                            .unwrap_or_default();
                        out.push_str(&format!("![{}]({})", alt, src));
                    }
                    "ticket_link" => {
                        out.push_str(&ticket_link_markdown(child_elem, txn, numbers));
                    }
                    "mention" => {
                        let name = child_elem
                            .get_attribute(txn, "name")
                            .map(|v| v.to_string(txn))
                            .unwrap_or_else(|| "unknown".to_string());
                        out.push_str(&format!("@{}", name));
                    }
                    _ => {
                        // For marks like strong, em, code, link
                        let inner = collect_inline_children(child_elem, txn, numbers);
                        out.push_str(&wrap_with_mark(&tag, child_elem, txn, &inner));
                    }
                }
            }
            XmlOut::Fragment(frag) => {
                for fc in frag.children(txn) {
                    if let XmlOut::Text(t) = &fc {
                        out.push_str(&get_text_safe(t, txn));
                    }
                }
            }
        }
    }
    out
}

/// Wrap text with markdown formatting based on the mark/tag name
fn wrap_with_mark(
    tag: &str,
    elem: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    inner: &str,
) -> String {
    match tag {
        "strong" | "b" => format!("**{}**", inner),
        "em" | "i" => format!("*{}*", inner),
        "code" => format!("`{}`", inner),
        "link" | "a" => {
            let href = elem
                .get_attribute(txn, "href")
                .map(|v| v.to_string(txn))
                .unwrap_or_default();
            format!("[{}]({})", inner, href)
        }
        _ => inner.to_string(),
    }
}

/// Collect raw text content (for code blocks, no markdown processing)
fn collect_raw_text(elem: &yrs::XmlElementRef, txn: &yrs::Transaction) -> String {
    let mut out = String::new();
    for child in elem.children(txn) {
        match &child {
            XmlOut::Text(text_ref) => {
                out.push_str(&get_text_safe(text_ref, txn));
            }
            XmlOut::Element(child_elem) => {
                let tag_ref = child_elem.tag();
                if tag_ref.as_ref() == "br" || tag_ref.as_ref() == "hard_break" {
                    out.push('\n');
                } else {
                    out.push_str(&collect_raw_text(child_elem, txn));
                }
            }
            XmlOut::Fragment(frag) => {
                for fc in frag.children(txn) {
                    if let XmlOut::Text(t) = &fc {
                        out.push_str(&get_text_safe(t, txn));
                    }
                }
            }
        }
    }
    out
}

/// Collect block-level children of an element
fn collect_block_children(
    elem: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    list_depth: usize,
    mut resolver: Option<EmbedResolver<'_>>,
    visited: &mut HashSet<Uuid>,
    embed_depth: usize,
    locale: Option<&LanguageIdentifier>,
    numbers: &TicketNumbers,
) -> String {
    let mut out = String::new();
    for child in elem.children(txn) {
        match &child {
            XmlOut::Element(child_elem) => {
                let tag = child_elem.tag().to_string();
                out.push_str(&element_to_markdown(
                    &tag,
                    child_elem,
                    txn,
                    list_depth,
                    resolver.as_mut().map(EmbedResolver::reborrow),
                    visited,
                    embed_depth,
                    locale,
                    numbers,
                ));
            }
            XmlOut::Text(text_ref) => {
                out.push_str(&get_text_safe(text_ref, txn));
            }
            XmlOut::Fragment(frag) => {
                for fc in frag.children(txn) {
                    if let XmlOut::Text(t) = &fc {
                        out.push_str(&get_text_safe(t, txn));
                    }
                }
            }
        }
    }
    out
}

/// Collect list item content (first paragraph inline, subsequent blocks as sub-content)
fn collect_list_item_content(
    li: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    list_depth: usize,
    mut resolver: Option<EmbedResolver<'_>>,
    visited: &mut HashSet<Uuid>,
    embed_depth: usize,
    locale: Option<&LanguageIdentifier>,
    numbers: &TicketNumbers,
) -> String {
    let mut out = String::new();
    let mut first = true;
    for child in li.children(txn) {
        if let XmlOut::Element(child_elem) = &child {
            let tag = child_elem.tag().to_string();
            if first && tag == "paragraph" {
                out.push_str(&collect_inline_children(child_elem, txn, numbers));
                first = false;
            } else if tag == "bullet_list" || tag == "ordered_list" {
                out.push('\n');
                out.push_str(&element_to_markdown(
                    &tag,
                    child_elem,
                    txn,
                    list_depth + 1,
                    resolver.as_mut().map(EmbedResolver::reborrow),
                    visited,
                    embed_depth,
                    locale,
                    numbers,
                ));
            } else {
                out.push_str(&element_to_markdown(
                    &tag,
                    child_elem,
                    txn,
                    list_depth,
                    resolver.as_mut().map(EmbedResolver::reborrow),
                    visited,
                    embed_depth,
                    locale,
                    numbers,
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::types::xml::XmlIn;
    use yrs::{StateVector, XmlElementPrelim};

    /// A document whose one paragraph links ticket 42.
    fn linking_ticket_42() -> Doc {
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment("prosemirror");
            let mut link = XmlElementPrelim::new("ticket_link", Vec::<XmlIn>::new());
            link.attributes.insert("ticketId".into(), "42".to_string());
            fragment.push_back(
                &mut txn,
                XmlElementPrelim::new("paragraph", vec![link.into()]),
            );
        }
        doc
    }

    #[test]
    fn a_ticket_link_quotes_the_tickets_number() {
        let doc = linking_ticket_42();
        let txn = doc.transact();
        let paragraph = txn
            .get_xml_fragment("prosemirror")
            .and_then(|f| f.get(&txn, 0))
            .expect("paragraph");
        let numbers = TicketNumbers::from([(42, 7)]);
        assert_eq!(
            node_to_markdown(&paragraph, &txn, 0, &numbers),
            "[Ticket #7](/tickets/7)\n"
        );
    }

    #[test]
    fn a_ticket_link_without_a_number_goes_by_id() {
        let doc = linking_ticket_42();
        let update = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        assert_eq!(
            yjs_to_markdown(&update).as_deref(),
            Some("[Ticket](/tickets/id/42)")
        );
    }
}
