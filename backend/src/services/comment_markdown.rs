//! Markdown a caller posts as a comment, as the comment editor's HTML.
//!
//! The web and mobile composer write HTML; scripts, integrations and chat
//! bridges write Markdown. Converting at the door means everything after
//! it (the thread, email replies, mentions of other tickets, search) sees
//! one format.
//!
//! The output uses the comment editor's elements: paragraphs, headings,
//! lists, quotes, code, rules, links, bold, italic, and its ticket-link
//! span. Comments hold no tables (they go out by email, where tables
//! render unevenly), so a table becomes a paragraph per row with its cells
//! joined by " · ", as the composer does with a pasted table. Raw HTML in
//! the Markdown stays visible as text, and the result is sanitised to that
//! element set regardless.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use once_cell::sync::Lazy;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use regex::Regex;

/// Joins a table row's cells, as `CELL_SEPARATOR` in the composer's
/// `tableText.ts` does.
const CELL_SEPARATOR: &str = " · ";

/// A ticket number (`#208`) or a bare web address in running text.
static TEXT_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://[^\s<>]+|#(\d{1,9})\b").unwrap());

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
}

/// Every ticket number written as `#n` in running text: what
/// [`to_comment_html`] needs resolved to ticket ids.
pub fn ticket_numbers(markdown: &str) -> Vec<i32> {
    let mut numbers = HashSet::new();
    let mut code_depth = 0usize;
    let mut link_depth = 0usize;
    let mut previous_char = None;
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth = code_depth.saturating_sub(1),
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => link_depth += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => link_depth = link_depth.saturating_sub(1),
            Event::Text(text) if code_depth == 0 && link_depth == 0 => {
                for token in tokens(&text, previous_char) {
                    if let Token::Ticket(n) = token {
                        numbers.insert(n);
                    }
                }
                previous_char = text.chars().last();
                continue;
            }
            _ => {}
        }
        previous_char = None;
    }
    let mut out: Vec<i32> = numbers.into_iter().collect();
    out.sort_unstable();
    out
}

/// The comment editor's HTML for `markdown`. `ticket_ids` maps the ticket
/// numbers this workspace has to their ids; a `#n` with no entry stays text.
pub fn to_comment_html(markdown: &str, ticket_ids: &HashMap<i32, i32>) -> String {
    let mut w = Writer {
        out: String::with_capacity(markdown.len() + markdown.len() / 4),
        ticket_ids,
        code_depth: 0,
        links: Vec::new(),
        image_depth: 0,
        cell_index: 0,
        previous_char: None,
    };
    for event in Parser::new_ext(markdown, options()) {
        w.event(event);
    }
    SANITISER.clean(&w.out).to_string()
}

struct Writer<'m> {
    out: String,
    ticket_ids: &'m HashMap<i32, i32>,
    code_depth: usize,
    /// Whether each open link was kept (an allowed destination).
    links: Vec<bool>,
    /// Inside an image, whose text is its alt text.
    image_depth: usize,
    cell_index: usize,
    /// The last character of the previous text event in the same run, so
    /// `#` after a letter split across events isn't read as a reference.
    previous_char: Option<char>,
}

impl Writer<'_> {
    fn event(&mut self, event: Event<'_>) {
        if !matches!(event, Event::Text(_)) {
            self.previous_char = None;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(text) => {
                if self.code_depth > 0 || self.image_depth > 0 || !self.links.is_empty() {
                    escape_into(&mut self.out, &text);
                } else {
                    self.running_text(&text);
                }
                self.previous_char = text.chars().last();
            }
            Event::Code(code) => {
                self.out.push_str("<code>");
                escape_into(&mut self.out, &code);
                self.out.push_str("</code>");
            }
            // Raw HTML is shown as typed, never interpreted.
            Event::Html(html) | Event::InlineHtml(html) => escape_into(&mut self.out, &html),
            Event::SoftBreak | Event::HardBreak => self.out.push_str("<br>"),
            Event::Rule => self.out.push_str("<hr>"),
            // Not enabled; kept as text should a future option produce them.
            Event::InlineMath(t) | Event::DisplayMath(t) | Event::FootnoteReference(t) => {
                escape_into(&mut self.out, &t)
            }
            Event::TaskListMarker(done) => self.out.push_str(if done { "☑ " } else { "☐ " }),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.out.push_str("<p>"),
            Tag::Heading { level, .. } => {
                let _ = write!(self.out, "<h{}>", heading_number(level));
            }
            Tag::BlockQuote(_) => self.out.push_str("<blockquote>"),
            Tag::CodeBlock(_) => {
                self.code_depth += 1;
                self.out.push_str("<pre><code>");
            }
            Tag::List(Some(start)) if start != 1 => {
                let _ = write!(self.out, "<ol start=\"{start}\">");
            }
            Tag::List(Some(_)) => self.out.push_str("<ol>"),
            Tag::List(None) => self.out.push_str("<ul>"),
            Tag::Item => self.out.push_str("<li>"),
            Tag::Table(_) => {}
            Tag::TableHead | Tag::TableRow => {
                self.cell_index = 0;
                self.out.push_str("<p>");
            }
            Tag::TableCell => {
                if self.cell_index > 0 {
                    self.out.push_str(CELL_SEPARATOR);
                }
                self.cell_index += 1;
            }
            Tag::Emphasis => self.out.push_str("<em>"),
            Tag::Strong => self.out.push_str("<strong>"),
            Tag::Strikethrough => self.out.push_str("<s>"),
            Tag::Link { dest_url, .. } => {
                let kept = allowed_href(&dest_url);
                if kept {
                    self.out.push_str("<a href=\"");
                    escape_into(&mut self.out, &dest_url);
                    self.out.push_str("\">");
                }
                self.links.push(kept);
            }
            // An image's alt text, linked to the image when it's on the web.
            Tag::Image { dest_url, .. } => {
                self.image_depth += 1;
                let kept = allowed_href(&dest_url);
                if kept {
                    self.out.push_str("<a href=\"");
                    escape_into(&mut self.out, &dest_url);
                    self.out.push_str("\">");
                }
                self.links.push(kept);
            }
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => self.out.push_str("</p>"),
            TagEnd::Heading(level) => {
                let _ = write!(self.out, "</h{}>", heading_number(level));
            }
            TagEnd::BlockQuote(_) => self.out.push_str("</blockquote>"),
            TagEnd::CodeBlock => {
                self.code_depth = self.code_depth.saturating_sub(1);
                self.out.push_str("</code></pre>");
            }
            TagEnd::List(true) => self.out.push_str("</ol>"),
            TagEnd::List(false) => self.out.push_str("</ul>"),
            TagEnd::Item => self.out.push_str("</li>"),
            TagEnd::TableHead | TagEnd::TableRow => self.out.push_str("</p>"),
            TagEnd::Emphasis => self.out.push_str("</em>"),
            TagEnd::Strong => self.out.push_str("</strong>"),
            TagEnd::Strikethrough => self.out.push_str("</s>"),
            TagEnd::Image => {
                self.image_depth = self.image_depth.saturating_sub(1);
                self.close_link();
            }
            TagEnd::Link => self.close_link(),
            _ => {}
        }
    }

    fn close_link(&mut self) {
        if self.links.pop() == Some(true) {
            self.out.push_str("</a>");
        }
    }

    /// Text outside code and links: ticket numbers become ticket links and
    /// bare web addresses become links.
    fn running_text(&mut self, text: &str) {
        let mut last = 0;
        for m in TEXT_TOKEN_RE.captures_iter(text) {
            let whole = m.get(0).unwrap();
            let token = match m.get(1) {
                Some(number) => {
                    let before = text[..whole.start()]
                        .chars()
                        .last()
                        .or(if whole.start() == 0 {
                            self.previous_char
                        } else {
                            None
                        });
                    if !reference_may_follow(before) {
                        continue;
                    }
                    let Ok(n) = number.as_str().parse::<i32>() else {
                        continue;
                    };
                    let Some(&id) = self.ticket_ids.get(&n) else {
                        continue;
                    };
                    escape_into(&mut self.out, &text[last..whole.start()]);
                    let _ = write!(
                        self.out,
                        "<span data-ticket-link=\"true\" data-ticket-id=\"{id}\" data-href=\"/tickets/id/{id}\">#{n}</span>"
                    );
                    last = whole.end();
                    continue;
                }
                None => whole
                    .as_str()
                    .trim_end_matches(|c: char| ".,:;!?)]'\"".contains(c)),
            };
            escape_into(&mut self.out, &text[last..whole.start()]);
            self.out.push_str("<a href=\"");
            escape_into(&mut self.out, token);
            self.out.push_str("\">");
            escape_into(&mut self.out, token);
            self.out.push_str("</a>");
            last = whole.start() + token.len();
        }
        escape_into(&mut self.out, &text[last..]);
    }
}

enum Token {
    Ticket(i32),
    Url,
}

/// The ticket numbers and web addresses in a text event, for
/// [`ticket_numbers`]. Mirrors [`Writer::running_text`]'s rules.
fn tokens(text: &str, previous_char: Option<char>) -> Vec<Token> {
    TEXT_TOKEN_RE
        .captures_iter(text)
        .filter_map(|m| {
            let whole = m.get(0)?;
            match m.get(1) {
                Some(number) => {
                    let before = text[..whole.start()]
                        .chars()
                        .last()
                        .or(if whole.start() == 0 {
                            previous_char
                        } else {
                            None
                        });
                    reference_may_follow(before)
                        .then(|| number.as_str().parse().ok().map(Token::Ticket))
                        .flatten()
                }
                None => Some(Token::Url),
            }
        })
        .collect()
}

/// `#208` is a reference at the start of text or after a space or an
/// opening bracket, not inside a word or an anchor like `page#12`.
fn reference_may_follow(before: Option<char>) -> bool {
    match before {
        None => true,
        Some(c) => c.is_whitespace() || "([{".contains(c),
    }
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn allowed_href(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// The comment editor's elements and nothing else, whatever the writer
/// above produced.
static SANITISER: Lazy<ammonia::Builder<'static>> = Lazy::new(|| {
    let mut b = ammonia::Builder::default();
    b.tags(
        [
            "p",
            "br",
            "strong",
            "em",
            "s",
            "code",
            "pre",
            "blockquote",
            "ul",
            "ol",
            "li",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "hr",
            "a",
            "span",
        ]
        .into_iter()
        .collect::<HashSet<_>>(),
    );
    b.generic_attributes(HashSet::new());
    b.tag_attributes(
        [
            ("a", ["href"].into_iter().collect::<HashSet<_>>()),
            ("ol", ["start"].into_iter().collect()),
            (
                "span",
                ["data-ticket-link", "data-ticket-id", "data-href"]
                    .into_iter()
                    .collect(),
            ),
        ]
        .into_iter()
        .collect(),
    );
    b.url_schemes(["http", "https", "mailto"].into_iter().collect());
    b.url_relative(ammonia::UrlRelative::Deny);
    b
});

#[cfg(test)]
mod tests {
    use super::*;

    fn html(markdown: &str) -> String {
        to_comment_html(markdown, &HashMap::from([(208, 4021), (7, 33)]))
    }

    #[test]
    fn formats_what_the_editor_can_hold() {
        assert_eq!(
            html("## Status\n\n**Bold**, *italic*, ~~gone~~ and `code`.\n\n- one\n- two\n\n3. three\n4. four\n\n> quoted\n\n---"),
            "<h2>Status</h2><p><strong>Bold</strong>, <em>italic</em>, <s>gone</s> and <code>code</code>.</p>\
             <ul><li>one</li><li>two</li></ul><ol start=\"3\"><li>three</li><li>four</li></ol>\
             <blockquote><p>quoted</p></blockquote><hr>"
        );
    }

    #[test]
    fn keeps_line_breaks_inside_a_paragraph() {
        assert_eq!(
            html("first line\nsecond line"),
            "<p>first line<br>second line</p>"
        );
    }

    #[test]
    fn keeps_code_blocks_verbatim() {
        assert_eq!(
            html("```rust\nlet x = a < b && #208;\n```"),
            "<pre><code>let x = a &lt; b &amp;&amp; #208;\n</code></pre>"
        );
    }

    #[test]
    fn links_web_and_mail_addresses_only() {
        assert_eq!(
            html("[site](https://nosdesk.dev), [mail](mailto:a@b.c), [bad](javascript:alert(1)), [rel](/x)"),
            "<p><a href=\"https://nosdesk.dev\" rel=\"noopener noreferrer\">site</a>, \
             <a href=\"mailto:a@b.c\" rel=\"noopener noreferrer\">mail</a>, bad, rel</p>"
        );
    }

    #[test]
    fn links_bare_web_addresses_without_trailing_punctuation() {
        assert_eq!(
            html("See https://nosdesk.dev/docs. Or (https://example.com/a?b=1)."),
            "<p>See <a href=\"https://nosdesk.dev/docs\" rel=\"noopener noreferrer\">https://nosdesk.dev/docs</a>. \
             Or (<a href=\"https://example.com/a?b=1\" rel=\"noopener noreferrer\">https://example.com/a?b=1</a>).</p>"
        );
    }

    #[test]
    fn turns_known_ticket_numbers_into_ticket_links() {
        assert_eq!(
            html("Same as #208 and (#7), not #999, page#7 or `#208`."),
            "<p>Same as <span data-ticket-link=\"true\" data-ticket-id=\"4021\" data-href=\"/tickets/id/4021\">#208</span> \
             and (<span data-ticket-link=\"true\" data-ticket-id=\"33\" data-href=\"/tickets/id/33\">#7</span>), \
             not #999, page#7 or <code>#208</code>.</p>"
        );
    }

    #[test]
    fn finds_the_ticket_numbers_to_resolve() {
        assert_eq!(
            ticket_numbers(
                "#208, #7 and #208 again; not `#5`, [#6](https://x.y), word#9 or\n```\n#4\n```"
            ),
            vec![7, 208]
        );
    }

    #[test]
    fn shows_raw_html_as_text() {
        assert_eq!(
            html("<script>alert(1)</script>\n\nHi <b onclick=\"x()\">there</b>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;\n<p>Hi &lt;b onclick=\"x()\"&gt;there&lt;/b&gt;</p>"
        );
    }

    #[test]
    fn flattens_a_table_to_a_paragraph_per_row() {
        assert_eq!(
            html("| Asset | State |\n|---|---|\n| Laptop | **In use** |\n| Phone | Spare |"),
            "<p>Asset · State</p><p>Laptop · <strong>In use</strong></p><p>Phone · Spare</p>"
        );
    }

    #[test]
    fn keeps_an_image_as_its_alt_text() {
        assert_eq!(
            html("![diagram](https://x.y/d.png) and ![local](d.png)"),
            "<p><a href=\"https://x.y/d.png\" rel=\"noopener noreferrer\">diagram</a> and local</p>"
        );
    }
}
