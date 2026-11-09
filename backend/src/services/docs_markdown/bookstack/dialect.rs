//! BookStack's HTML, as its editors write it and its exporter renders it.
//!
//! - Callouts are `p.callout` with `info`, `success`, `warning` or
//!   `danger`: a quote with the kind in bold.
//! - Collapsible blocks are `<details><summary>`: the summary in bold, the
//!   content shown.
//! - Checklist items are `li.task-list-item` holding a disabled
//!   `input[type=checkbox]`, checked by its `checked` attribute (the
//!   WYSIWYG editor's own copy keeps `checked` on the item).
//! - draw.io diagrams are `div[drawio-diagram]` around the PNG.
//! - Every block carries a `bkmrk-` id, and the WYSIWYG editor wraps text
//!   in styled `<span>`s: both dropped, the text kept.

use super::super::html::{Dialect, ElementView, Rewrite, Selector};
use super::super::NoteCode;
use super::SCHEME;

pub struct BookstackDialect;

/// The callout kinds BookStack styles, and their labels.
const CALLOUTS: [(&str, &str); 4] = [
    ("info", "Info"),
    ("success", "Success"),
    ("warning", "Warning"),
    ("danger", "Danger"),
];

impl Dialect for BookstackDialect {
    fn classify(&self, element: &ElementView) -> Option<Rewrite> {
        match element.tag() {
            "p" | "div" | "blockquote" if element.has_class("callout") => {
                let label = CALLOUTS
                    .iter()
                    .find(|(class, _)| element.has_class(class))
                    .map(|(_, label)| *label)
                    .unwrap_or("Note");
                Some(Rewrite::Callout {
                    label: label.to_string(),
                })
            }
            "details" => Some(Rewrite::Expand {
                title: element
                    .children()
                    .into_iter()
                    .find(|c| c.tag() == "summary")
                    .map(|summary| summary.text()),
            }),
            // Its text is the title the details carry.
            "summary" if element.parent().is_some_and(|p| p.tag() == "details") => {
                Some(Rewrite::Remove(None))
            }
            "li" if element.has_class("task-list-item") => Some(Rewrite::Task {
                checked: element.attr("checked").is_some()
                    || element
                        .children()
                        .iter()
                        .any(|c| c.tag() == "input" && c.attr("checked").is_some()),
            }),
            // The checklist's box: the item carries its state.
            "input"
                if element
                    .closest(&Selector::Class("task-list-item"))
                    .is_some() =>
            {
                Some(Rewrite::Remove(None))
            }
            "div" if element.attr("drawio-diagram").is_some() => {
                Some(Rewrite::Unwrap(Some(NoteCode::DiagramsAsImages)))
            }
            // The editor links each image to its full size: the image stays,
            // the link to it goes.
            "a" if element.attr("href").is_some_and(|h| is_image_ref(&h)) => {
                Some(Rewrite::Unwrap(None))
            }
            "div" | "span" | "section" => Some(Rewrite::Unwrap(None)),
            _ => None,
        }
    }
}

/// `bsexport://{part}/image/{id}`, as [`super::rewrite_refs`] writes it.
fn is_image_ref(href: &str) -> bool {
    href.trim()
        .strip_prefix(SCHEME)
        .and_then(|rest| rest.strip_prefix("://"))
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, rest)| rest.starts_with("image/"))
}
