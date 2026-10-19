//! Source profiles: which tool wrote an archive, and the rules that switches
//! on.
//!
//! A profile only switches rules on. Every rule is safe on any input, so a
//! wrong guess degrades an import rather than breaking it, and an admin can
//! force a profile. Detection looks at names first (a manifest, a config
//! file, a tool folder) and reads text only to tell Nosdesk v1, Obsidian and
//! Notion apart from a plain folder.
//!
//! BookStack and Confluence exports are recognised and refused: they are
//! HTML with their own trees and macros, and arrive in a later release.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::front_matter::{self, Value, YamlLimits};
use super::links::{notion_id, parent};
use super::{NoteCode, Notes};
use crate::services::imports::refusal::Refused;

/// Which tool wrote the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    /// A Nosdesk documentation export with a `nosdesk-docs.json` manifest.
    NosdeskV2,
    /// The browser-built Nosdesk export of 1.1.2 and earlier.
    NosdeskV1,
    Notion,
    Obsidian,
    Gitbook,
    Mkdocs,
    Docusaurus,
    Outline,
    /// A folder of Markdown files, or loose files.
    Plain,
}

impl ProfileKind {
    pub const ALL: [ProfileKind; 9] = [
        ProfileKind::NosdeskV2,
        ProfileKind::NosdeskV1,
        ProfileKind::Notion,
        ProfileKind::Obsidian,
        ProfileKind::Gitbook,
        ProfileKind::Mkdocs,
        ProfileKind::Docusaurus,
        ProfileKind::Outline,
        ProfileKind::Plain,
    ];

    /// The rules this profile switches on.
    pub fn rules(self) -> Rules {
        let mut rules = Rules {
            number_prefixes: true,
            title_heading: true,
            ..Rules::default()
        };
        match self {
            ProfileKind::NosdeskV2 => {
                rules.number_prefixes = false;
                rules.title_heading = false;
            }
            ProfileKind::NosdeskV1 => rules.number_prefixes = false,
            ProfileKind::Notion => rules.notion_ids = true,
            ProfileKind::Obsidian => {
                rules.wikilinks = true;
                rules.obsidian_comments = true;
                rules.folder_note_inside = true;
            }
            ProfileKind::Gitbook => rules.gitbook_tags = true,
            ProfileKind::Mkdocs => rules.mkdocs_admonitions = true,
            ProfileKind::Docusaurus => {
                rules.docusaurus_admonitions = true;
                rules.mdx = true;
                rules.folder_note_inside = true;
            }
            ProfileKind::Outline | ProfileKind::Plain => {}
        }
        rules
    }
}

/// The rules a profile switches on. Each is safe on any input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rules {
    /// `[[Note]]`, `![[image.png|300]]`.
    pub wikilinks: bool,
    /// `%%comments%%` removed.
    pub obsidian_comments: bool,
    /// Names ending in a Notion 32-hex id have it stripped.
    pub notion_ids: bool,
    /// `01-`, `1. `, `010_` prefixes order siblings and are stripped from
    /// titles.
    pub number_prefixes: bool,
    /// The first H1 names the page, and is removed when it repeats the
    /// title.
    pub title_heading: bool,
    /// `Folder/Folder.md` is the folder's page.
    pub folder_note_inside: bool,
    /// `!!! note "Title"` admonitions become callouts.
    pub mkdocs_admonitions: bool,
    /// `:::tip Title` admonitions become callouts.
    pub docusaurus_admonitions: bool,
    /// `{% hint %}` becomes a callout, `{% embed url %}` a link, other
    /// template tags are removed.
    pub gitbook_tags: bool,
    /// MDX `import`/`export` lines and JSX tag lines are removed.
    pub mdx: bool,
}

/// What detection found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub kind: ProfileKind,
    /// The folder the source's files sit under (a wrapper folder the zip
    /// added, or empty).
    pub base: String,
}

/// What detection reads: the archive's names, and a way to read small text
/// files and the Markdown already loaded.
pub struct Listing<'a> {
    /// Readable file keys.
    pub files: &'a [String],
    /// Keys set aside by the archive's ignore rules (`.obsidian/...`).
    pub ignored: &'a [String],
    /// Read a small text file by key.
    pub read: &'a mut dyn FnMut(&str) -> Option<String>,
    /// The Markdown files' text, by key.
    pub markdown: &'a HashMap<String, String>,
}

/// The manifest a Nosdesk export v2 writes.
pub const MANIFEST: &str = "nosdesk-docs.json";

pub fn is_markdown(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".mdx")
}

/// The single folder everything sits in, when the zip wraps its content in
/// one; otherwise empty.
pub fn wrapper_folder(files: &[String]) -> String {
    let mut first: Option<&str> = None;
    for key in files {
        let Some((top, _)) = key.split_once('/') else {
            return String::new();
        };
        match first {
            None => first = Some(top),
            Some(seen) if seen == top => {}
            Some(_) => return String::new(),
        }
    }
    first.unwrap_or("").to_string()
}

fn under<'a>(base: &str, key: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        Some(key)
    } else {
        key.strip_prefix(base)?.strip_prefix('/')
    }
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

/// The refusal for an export this release can't read.
fn later(source: &str) -> Refused {
    Refused::new(
        "docs_import_source_later",
        format!(
            "This is a {source} export. BookStack and Confluence exports are coming in a later update."
        ),
    )
    .arg("source", source)
}

/// Which tool wrote the archive.
pub fn detect(listing: &mut Listing<'_>) -> Result<Detection, Refused> {
    let wrapper = wrapper_folder(listing.files);
    // An Outline export of one collection is one top-level folder: keep it.
    let outline_single = !wrapper.is_empty() && is_outline(listing.files, "");
    let base = if outline_single {
        String::new()
    } else {
        wrapper
    };
    let has = |name: &str| listing.files.iter().any(|k| *k == join(&base, name));
    let relative: Vec<&str> = listing
        .files
        .iter()
        .filter_map(|k| under(&base, k))
        .collect();
    let markdown: Vec<&str> = listing
        .files
        .iter()
        .map(String::as_str)
        .filter(|k| is_markdown(k))
        .collect();

    let found = |kind| {
        Ok(Detection {
            kind,
            base: base.clone(),
        })
    };

    if has(MANIFEST) {
        let manifest = (listing.read)(&join(&base, MANIFEST)).and_then(|t| read_manifest(&t));
        if manifest.is_some() {
            return found(ProfileKind::NosdeskV2);
        }
    }
    if has("data.json") {
        if let Some(text) = (listing.read)(&join(&base, "data.json")) {
            if is_bookstack(&text) {
                return Err(later("BookStack"));
            }
        }
    }
    if markdown.is_empty() {
        if is_confluence(&relative, listing, &base) {
            return Err(later("Confluence"));
        }
        return found(ProfileKind::Plain);
    }
    if is_nosdesk_v1(&markdown, &base, listing.markdown) {
        return found(ProfileKind::NosdeskV1);
    }
    if has("SUMMARY.md") || has(".gitbook.yaml") {
        return found(ProfileKind::Gitbook);
    }
    if has("mkdocs.yml") || has("mkdocs.yaml") {
        return found(ProfileKind::Mkdocs);
    }
    let docusaurus_config = ["js", "ts", "mjs", "cjs"]
        .iter()
        .any(|ext| has(&format!("docusaurus.config.{ext}")));
    let category = relative.iter().any(|k| {
        let name = k.rsplit('/').next().unwrap_or(k);
        matches!(
            name,
            "_category_.json" | "_category_.yml" | "_category_.yaml"
        )
    });
    if docusaurus_config || category {
        return found(ProfileKind::Docusaurus);
    }
    let obsidian_folder = listing
        .ignored
        .iter()
        .any(|k| k.split('/').any(|s| s == ".obsidian"));
    if obsidian_folder || mostly_wikilinks(&markdown, listing.markdown) {
        return found(ProfileKind::Obsidian);
    }
    if mostly_notion_names(&markdown) {
        return found(ProfileKind::Notion);
    }
    if is_outline(listing.files, &base) {
        return found(ProfileKind::Outline);
    }
    found(ProfileKind::Plain)
}

/// A manifest this importer reads.
pub fn read_manifest(text: &str) -> Option<Manifest> {
    let manifest: Manifest = serde_json::from_str(text).ok()?;
    (manifest.format == "nosdesk-docs" && manifest.format_version >= 2).then_some(manifest)
}

/// BookStack's portable zip: `data.json` holding a book, chapter or page
/// beside the instance it came from.
fn is_bookstack(text: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => {
            value.get("instance").is_some()
                && ["book", "chapter", "page"]
                    .iter()
                    .any(|k| value.get(k).is_some())
        }
        Err(_) => false,
    }
}

/// Confluence's HTML export: `index.html` and pages as HTML, with an
/// `attachments/` folder or Atlassian's markup in the index.
fn is_confluence(relative: &[&str], listing: &mut Listing<'_>, base: &str) -> bool {
    if !relative.contains(&"index.html") {
        return false;
    }
    if relative.iter().any(|k| k.starts_with("attachments/")) {
        return true;
    }
    (listing.read)(&join(base, "index.html"))
        .is_some_and(|t| t.contains("Confluence") || t.contains("atlassian"))
}

/// The 1.1.2 browser export: a `documentation/` folder whose files open
/// with front matter carrying `slug` and `status`.
fn is_nosdesk_v1(markdown: &[&str], base: &str, text: &HashMap<String, String>) -> bool {
    // The zip may hold `documentation/` itself, which then is the base.
    let prefix = if base.rsplit('/').next() == Some("documentation") {
        format!("{base}/")
    } else {
        join(base, "documentation/")
    };
    let inside: Vec<&&str> = markdown.iter().filter(|k| k.starts_with(&prefix)).collect();
    if inside.is_empty() || inside.len() * 2 < markdown.len() {
        return false;
    }
    let sample: Vec<&&str> = inside.iter().take(20).copied().collect();
    let marked = sample
        .iter()
        .filter(|k| {
            text.get(***k).is_some_and(|t| {
                let head: String = t.lines().take(12).collect::<Vec<_>>().join("\n");
                head.starts_with("---")
                    && head.lines().any(|l| l.starts_with("slug:"))
                    && head.lines().any(|l| l.starts_with("status:"))
            })
        })
        .count();
    marked * 2 > sample.len()
}

/// More than half of a sample of files hold a wikilink.
fn mostly_wikilinks(markdown: &[&str], text: &HashMap<String, String>) -> bool {
    let sample: Vec<&&str> = markdown.iter().take(50).collect();
    let with = sample
        .iter()
        .filter(|k| {
            text.get(***k).is_some_and(|t| {
                t.find("[[")
                    .is_some_and(|i| t[i + 2..].find("]]").is_some_and(|j| j > 0))
            })
        })
        .count();
    with * 2 > sample.len()
}

/// More than half of the Markdown files end their name in a Notion id.
fn mostly_notion_names(markdown: &[&str]) -> bool {
    let with = markdown
        .iter()
        .filter(|k| notion_id(super::links::file_stem(k)).is_some())
        .count();
    with * 2 > markdown.len()
}

/// Outline's export: top-level folders (collections) holding Markdown, at
/// least one with an `uploads/` folder, and no loose files at the top.
fn is_outline(files: &[String], base: &str) -> bool {
    let mut uploads = false;
    for key in files {
        let Some(rest) = under(base, key) else {
            continue;
        };
        let Some((top, inner)) = rest.split_once('/') else {
            return false;
        };
        if top.is_empty() {
            return false;
        }
        if inner.starts_with("uploads/") {
            uploads = true;
        }
    }
    uploads
}

// Source configs.

/// The manifest of a Nosdesk export v2, as the importer reads it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Manifest {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub workspace_uuid: Option<String>,
    #[serde(default)]
    pub collections: Vec<ManifestCollection>,
    #[serde(default)]
    pub pages: Vec<ManifestPage>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ManifestCollection {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ManifestPage {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub order: Option<i32>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// One entry of a GitBook `SUMMARY.md` or a MkDocs `nav`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavItem {
    pub title: Option<String>,
    /// The page's path as the config writes it, relative to the config's
    /// folder (GitBook) or `docs_dir` (MkDocs).
    pub path: Option<String>,
    pub children: Vec<NavItem>,
    /// A heading in `SUMMARY.md`: a group, which may become a collection.
    pub group: bool,
}

/// A GitBook `SUMMARY.md`: headings are groups, list links are pages, and
/// nested lists their children.
pub fn read_summary(markdown: &str) -> Vec<NavItem> {
    use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

    let mut roots: Vec<NavItem> = Vec::new();
    // Open items, outermost first; a group heading is the bottom when set.
    let mut stack: Vec<NavItem> = Vec::new();
    let mut heading: Option<String> = None;
    let mut item_text: Option<String> = None;

    fn close(stack: &mut Vec<NavItem>, roots: &mut Vec<NavItem>) {
        if let Some(item) = stack.pop() {
            match stack.last_mut() {
                Some(parent) => parent.children.push(item),
                None => roots.push(item),
            }
        }
    }

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) if level != HeadingLevel::H1 => {
                heading = Some(String::new());
            }
            Event::End(TagEnd::Heading(level)) if level != HeadingLevel::H1 => {
                let title = heading.take().unwrap_or_default().trim().to_string();
                while !stack.is_empty() {
                    close(&mut stack, &mut roots);
                }
                if !title.is_empty() {
                    stack.push(NavItem {
                        title: Some(title),
                        group: true,
                        ..NavItem::default()
                    });
                }
            }
            Event::Start(Tag::Item) => {
                stack.push(NavItem::default());
                item_text = Some(String::new());
            }
            Event::End(TagEnd::Item) => {
                if let (Some(text), Some(item)) = (item_text.take(), stack.last_mut()) {
                    if item.title.is_none() && !text.trim().is_empty() {
                        item.title = Some(text.trim().to_string());
                    }
                }
                close(&mut stack, &mut roots);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some(item) = stack.last_mut() {
                    if item.path.is_none() && !item.group {
                        item.path = Some(dest_url.to_string());
                        item_text = Some(String::new());
                    }
                }
            }
            Event::End(TagEnd::Link) => {
                if let (Some(text), Some(item)) = (item_text.take(), stack.last_mut()) {
                    if item.title.is_none() {
                        item.title = Some(text.trim().to_string());
                    }
                }
            }
            Event::Start(Tag::List(_)) => {
                // A nested list's items belong to the item holding it; text
                // read so far named that item.
                if let (Some(text), Some(item)) = (item_text.take(), stack.last_mut()) {
                    if item.title.is_none() && !text.trim().is_empty() && !item.group {
                        item.title = Some(text.trim().to_string());
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = heading.as_mut() {
                    h.push_str(&t);
                } else if let Some(text) = item_text.as_mut() {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    roots
}

/// A MkDocs config: `docs_dir`, `site_name` and `nav`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MkdocsConfig {
    pub docs_dir: String,
    pub site_name: Option<String>,
    pub nav: Option<Vec<NavItem>>,
}

pub fn read_mkdocs(text: &str) -> MkdocsConfig {
    let value = front_matter::parse_yaml(text, YamlLimits::CONFIG).unwrap_or(Value::Null);
    let docs_dir = value
        .get("docs_dir")
        .and_then(Value::to_text)
        .map(|d| {
            d.trim()
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_string()
        })
        .filter(|d| !d.is_empty() && !d.contains(".."))
        .unwrap_or_else(|| "docs".to_string());
    MkdocsConfig {
        docs_dir,
        site_name: value.get("site_name").and_then(Value::to_text),
        nav: match value.get("nav") {
            Some(Value::List(items)) => Some(nav_items(items)),
            _ => None,
        },
    }
}

fn nav_items(items: &[Value]) -> Vec<NavItem> {
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(path) => Some(NavItem {
                path: Some(path.clone()),
                ..NavItem::default()
            }),
            Value::Map(entries) => {
                let (title, value) = entries.first()?;
                match value {
                    Value::String(path) => Some(NavItem {
                        title: Some(title.clone()),
                        path: Some(path.clone()),
                        ..NavItem::default()
                    }),
                    Value::List(children) => Some(NavItem {
                        title: Some(title.clone()),
                        children: nav_items(children),
                        group: true,
                        ..NavItem::default()
                    }),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// A GitBook `.gitbook.yaml`: `root` and the `structure` file names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitbookConfig {
    pub root: String,
    pub readme: String,
    pub summary: String,
}

pub fn read_gitbook(text: Option<&str>) -> GitbookConfig {
    let value = text
        .and_then(|t| front_matter::parse_yaml(t, YamlLimits::CONFIG).ok())
        .unwrap_or(Value::Null);
    let clean = |s: String| {
        s.trim()
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string()
    };
    let structure = value.get("structure");
    GitbookConfig {
        root: value
            .get("root")
            .and_then(Value::to_text)
            .map(clean)
            .filter(|r| !r.contains(".."))
            .unwrap_or_default(),
        readme: structure
            .and_then(|s| s.get("readme"))
            .and_then(Value::to_text)
            .map(clean)
            .unwrap_or_else(|| "README.md".to_string()),
        summary: structure
            .and_then(|s| s.get("summary"))
            .and_then(Value::to_text)
            .map(clean)
            .unwrap_or_else(|| "SUMMARY.md".to_string()),
    }
}

/// A Docusaurus `_category_` file: its label and position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Category {
    pub label: Option<String>,
    pub position: Option<f64>,
}

pub fn read_category(key: &str, text: &str) -> Category {
    let value = if key.ends_with(".json") {
        serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .map(|v| json_value(&v))
            .unwrap_or(Value::Null)
    } else {
        front_matter::parse_yaml(text, YamlLimits::FRONT_MATTER).unwrap_or(Value::Null)
    };
    Category {
        label: value.get("label").and_then(Value::to_text),
        position: value.get("position").and_then(Value::as_f64),
    }
}

fn json_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .or_else(|| n.as_f64().map(Value::Float))
            .unwrap_or(Value::Null),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::List(items.iter().map(json_value).collect()),
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_value(v)))
                .collect(),
        ),
    }
}

/// The folder a key sits in, for `_category_` lookups.
pub fn category_folder(key: &str) -> &str {
    parent(key)
}

// Text rewrites, before parsing.

/// Rewrite source syntax with no Markdown equivalent into Markdown the
/// parser reads: callouts as `> [!type] Title` quotes, template and MDX
/// lines removed. Fenced code is left alone, and so is the front matter.
pub fn rewrite(text: &str, rules: &Rules, notes: &mut Notes) -> String {
    if !(rules.mkdocs_admonitions
        || rules.docusaurus_admonitions
        || rules.gitbook_tags
        || rules.mdx
        || rules.obsidian_comments)
    {
        return text.to_string();
    }
    let (front, body) = split_front_matter(text);
    let mut rewriter = Rewriter {
        rules,
        notes,
        out: Vec::new(),
        fence: None,
        quote_depth: 0,
        colons: Vec::new(),
        in_comment: false,
    };
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    rewriter.lines(&lines);
    let mut out = front.to_string();
    out.push_str(&rewriter.out.concat());
    out
}

/// The front matter block at the start of `text` (with its fences), and the
/// rest.
fn split_front_matter(text: &str) -> (&str, &str) {
    for fence in ["---", "+++"] {
        let mut lines = text.split_inclusive('\n');
        let Some(first) = lines.next() else {
            continue;
        };
        if first.trim_end() != fence {
            continue;
        }
        let mut end = first.len();
        for line in lines {
            end += line.len();
            let trimmed = line.trim_end();
            if trimmed == fence || (fence == "---" && trimmed == "...") {
                return (&text[..end], &text[end..]);
            }
        }
    }
    ("", text)
}

struct Rewriter<'a> {
    rules: &'a Rules,
    notes: &'a mut Notes,
    out: Vec<String>,
    /// The open code fence: its character and length.
    fence: Option<(char, usize)>,
    /// Docusaurus admonitions open, each a quote level.
    quote_depth: usize,
    /// The colon count of each open Docusaurus admonition.
    colons: Vec<usize>,
    /// Inside a `%%` comment that spans lines.
    in_comment: bool,
}

impl Rewriter<'_> {
    fn prefix(&self) -> String {
        "> ".repeat(self.quote_depth)
    }

    fn emit(&mut self, line: &str) {
        let prefix = self.prefix();
        self.out.push(format!("{prefix}{line}"));
    }

    fn lines(&mut self, lines: &[&str]) {
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            if let Some(fence) = self.fence {
                if closes_fence(trimmed, fence) {
                    self.fence = None;
                }
                self.emit(line);
                i += 1;
                continue;
            }
            if let Some(fence) = opens_fence(trimmed) {
                self.fence = Some(fence);
                self.emit(line);
                i += 1;
                continue;
            }
            if self.rules.obsidian_comments {
                let (kept, changed) = self.strip_comments(line);
                if changed {
                    if kept.trim().is_empty() {
                        i += 1;
                        continue;
                    }
                    self.emit(&kept);
                    i += 1;
                    continue;
                }
            }
            if self.rules.mkdocs_admonitions {
                if let Some(consumed) = self.mkdocs(lines, i) {
                    i += consumed;
                    continue;
                }
            }
            if self.rules.docusaurus_admonitions && self.docusaurus(trimmed) {
                i += 1;
                continue;
            }
            if self.rules.gitbook_tags {
                if let Some(rewritten) = self.gitbook(line) {
                    if let Some(rewritten) = rewritten {
                        self.emit(&rewritten);
                    }
                    i += 1;
                    continue;
                }
            }
            if self.rules.mdx && self.mdx_line(line) {
                self.notes.add(NoteCode::SourceSyntaxRemoved);
                i += 1;
                continue;
            }
            self.emit(line);
            i += 1;
        }
    }

    /// `%%...%%` removed, across lines. Returns the line left and whether
    /// anything was removed.
    fn strip_comments(&mut self, line: &str) -> (String, bool) {
        if !self.in_comment && !line.contains("%%") {
            return (line.to_string(), false);
        }
        let mut out = String::new();
        let mut rest = line;
        loop {
            if self.in_comment {
                match rest.find("%%") {
                    Some(end) => {
                        rest = &rest[end + 2..];
                        self.in_comment = false;
                    }
                    None => break,
                }
            } else {
                match rest.find("%%") {
                    Some(start) => {
                        out.push_str(&rest[..start]);
                        rest = &rest[start + 2..];
                        self.in_comment = true;
                        self.notes.add(NoteCode::SourceSyntaxRemoved);
                    }
                    None => {
                        out.push_str(rest);
                        break;
                    }
                }
            }
        }
        if !out.ends_with('\n') && line.ends_with('\n') {
            out.push('\n');
        }
        (out, true)
    }

    /// A MkDocs admonition at `lines[i]`: `!!! type "Title"` (or `???`,
    /// `???+`) and the lines indented four spaces under it. Returns the
    /// lines consumed.
    fn mkdocs(&mut self, lines: &[&str], i: usize) -> Option<usize> {
        let line = lines[i];
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("!!!")
            .or_else(|| trimmed.strip_prefix("???+"))
            .or_else(|| trimmed.strip_prefix("???"))?;
        let rest = rest.trim();
        let (kind, title) = match rest.split_once(char::is_whitespace) {
            Some((kind, title)) => (kind, title.trim()),
            None => (rest, ""),
        };
        if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let title = title.trim_matches('"');
        let pad = " ".repeat(indent);
        let header = if title.is_empty() {
            format!("{pad}> [!{kind}]\n")
        } else {
            format!("{pad}> [!{kind}] {title}\n")
        };
        self.emit(&header);
        let mut consumed = 1;
        let body_indent = indent + 4;
        let mut pending_blank = 0;
        for next in &lines[i + 1..] {
            let blank = next.trim().is_empty();
            let next_indent = next.len() - next.trim_start().len();
            if blank {
                pending_blank += 1;
                consumed += 1;
                continue;
            }
            if next_indent < body_indent {
                break;
            }
            for _ in 0..pending_blank {
                self.emit(&format!("{pad}>\n"));
            }
            pending_blank = 0;
            let body = &next[body_indent.min(next.len())..];
            self.emit(&format!("{pad}> {body}"));
            if !body.ends_with('\n') {
                self.emit("\n");
            }
            consumed += 1;
        }
        // Blank lines after the body stay blank lines.
        for _ in 0..pending_blank {
            self.emit("\n");
        }
        Some(consumed)
    }

    /// A Docusaurus admonition fence: `:::type Title`, `:::type[Title]`, or
    /// the closing `:::`. Returns whether the line was one.
    fn docusaurus(&mut self, trimmed: &str) -> bool {
        let colons = trimmed.chars().take_while(|c| *c == ':').count();
        if colons < 3 {
            return false;
        }
        let rest = trimmed[colons..].trim();
        if rest.is_empty() {
            if !self.colons.is_empty() {
                self.colons.pop();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                if self.quote_depth == 0 {
                    // End the quote before what follows.
                    self.out.push("\n".to_string());
                }
                return true;
            }
            return false;
        }
        let kind_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        if kind_len == 0 {
            return false;
        }
        let kind = &rest[..kind_len];
        let title = rest[kind_len..]
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim();
        let header = if title.is_empty() {
            format!("> [!{kind}]\n")
        } else {
            format!("> [!{kind}] {title}\n")
        };
        self.emit(&header);
        self.colons.push(colons);
        self.quote_depth += 1;
        true
    }

    /// A GitBook template tag line. `Some(None)` drops the line,
    /// `Some(Some(line))` replaces it, `None` leaves it alone.
    fn gitbook(&mut self, line: &str) -> Option<Option<String>> {
        let trimmed = line.trim();
        if trimmed.len() < 4 || !(trimmed.starts_with("{%") && trimmed.ends_with("%}")) {
            return None;
        }
        let inner = trimmed[2..trimmed.len() - 2].trim();
        let (tag, args) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        match tag {
            "hint" => {
                let style = attr_value(args, "style").unwrap_or_else(|| "info".to_string());
                self.emit(&format!("> [!{style}]\n"));
                self.quote_depth += 1;
                self.colons.push(0);
                Some(None)
            }
            "endhint" => {
                if self.colons.last() == Some(&0) {
                    self.colons.pop();
                    self.quote_depth = self.quote_depth.saturating_sub(1);
                    if self.quote_depth == 0 {
                        self.out.push("\n".to_string());
                    }
                }
                Some(None)
            }
            "embed" => match attr_value(args, "url") {
                Some(url) => Some(Some(format!("<{url}>\n"))),
                None => {
                    self.notes.add(NoteCode::SourceSyntaxRemoved);
                    Some(None)
                }
            },
            _ => {
                self.notes.add(NoteCode::SourceSyntaxRemoved);
                Some(None)
            }
        }
    }

    /// An MDX `import`/`export` line or a line holding only a JSX tag.
    fn mdx_line(&self, line: &str) -> bool {
        let trimmed = line.trim();
        let module_line = !line.starts_with([' ', '\t'])
            && ((trimmed.starts_with("import ")
                && (trimmed.contains(" from ")
                    || trimmed[7..].trim_start().starts_with(['\'', '"'])))
                || trimmed.starts_with("export "));
        let jsx = {
            let body = trimmed
                .strip_prefix("</")
                .or_else(|| trimmed.strip_prefix('<'));
            body.is_some_and(|b| b.starts_with(|c: char| c.is_ascii_uppercase()))
                && trimmed.ends_with('>')
        };
        module_line || jsx
    }
}

/// A `key="value"` argument's value.
fn attr_value(args: &str, key: &str) -> Option<String> {
    let start = args.find(&format!("{key}="))? + key.len() + 1;
    let rest = &args[start..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)?;
        Some(rest[1..1 + end].to_string())
    } else {
        Some(rest.split_whitespace().next()?.to_string())
    }
}

fn opens_fence(trimmed: &str) -> Option<(char, usize)> {
    let c = trimmed.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|x| *x == c).count();
    (len >= 3).then_some((c, len))
}

fn closes_fence(trimmed: &str, (c, len): (char, usize)) -> bool {
    let run = trimmed.chars().take_while(|x| *x == c).count();
    run >= len && trimmed[run * c.len_utf8()..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewritten(text: &str, kind: ProfileKind) -> (String, Notes) {
        let mut notes = Notes::default();
        let out = rewrite(text, &kind.rules(), &mut notes);
        (out, notes)
    }

    #[test]
    fn mkdocs_admonitions_become_callout_quotes() {
        let (out, _) = rewritten(
            "Intro\n\n!!! warning \"Back up first\"\n    Copy the files.\n\n    Then restart.\n\nAfter\n",
            ProfileKind::Mkdocs,
        );
        assert_eq!(
            out,
            "Intro\n\n> [!warning] Back up first\n> Copy the files.\n>\n> Then restart.\n\nAfter\n"
        );
    }

    #[test]
    fn docusaurus_admonitions_nest() {
        let (out, _) = rewritten(
            ":::tip Remember\nOuter\n::::note\nInner\n::::\n:::\nAfter\n",
            ProfileKind::Docusaurus,
        );
        assert_eq!(
            out,
            "> [!tip] Remember\n> Outer\n> > [!note]\n> > Inner\n\nAfter\n"
        );
    }

    #[test]
    fn gitbook_tags_become_callouts_links_or_nothing() {
        let (out, notes) = rewritten(
            "{% hint style=\"warning\" %}\nCareful.\n{% endhint %}\n\n{% embed url=\"https://e.example/v\" %}\n{% tabs %}\nText\n",
            ProfileKind::Gitbook,
        );
        assert_eq!(
            out,
            "> [!warning]\n> Careful.\n\n\n<https://e.example/v>\nText\n"
        );
        assert_eq!(notes.count(NoteCode::SourceSyntaxRemoved), 1);
    }

    #[test]
    fn mdx_lines_and_obsidian_comments_are_removed_outside_code() {
        let (out, notes) = rewritten(
            "import Tabs from '@theme/Tabs';\n\n<Tabs>\nKept\n</Tabs>\n\n```js\nimport x from 'y';\n```\n",
            ProfileKind::Docusaurus,
        );
        assert_eq!(out, "\nKept\n\n```js\nimport x from 'y';\n```\n");
        assert_eq!(notes.count(NoteCode::SourceSyntaxRemoved), 3);

        let (out, _) = rewritten("a %%hidden%% b\n%%\nall\n%%\nc\n", ProfileKind::Obsidian);
        assert_eq!(out, "a  b\nc\n");
    }

    #[test]
    fn front_matter_is_left_alone() {
        let (out, _) = rewritten("---\ntitle: \"%%x%%\"\n---\nBody\n", ProfileKind::Obsidian);
        assert_eq!(out, "---\ntitle: \"%%x%%\"\n---\nBody\n");
    }

    #[test]
    fn a_summary_reads_groups_pages_and_children() {
        let nav = read_summary(
            "# Summary\n\n* [Welcome](README.md)\n\n## Guides\n\n* [Setup](guides/setup.md)\n  * [VPN](guides/vpn.md)\n* Loose title\n",
        );
        assert_eq!(nav.len(), 2);
        assert_eq!(nav[0].title.as_deref(), Some("Welcome"));
        assert_eq!(nav[0].path.as_deref(), Some("README.md"));
        assert!(nav[1].group);
        assert_eq!(nav[1].children[0].title.as_deref(), Some("Setup"));
        assert_eq!(
            nav[1].children[0].children[0].path.as_deref(),
            Some("guides/vpn.md")
        );
        assert_eq!(nav[1].children[1].title.as_deref(), Some("Loose title"));
        assert_eq!(nav[1].children[1].path, None);
    }

    #[test]
    fn mkdocs_nav_nests() {
        let config = read_mkdocs(
            "site_name: IT Handbook\ndocs_dir: content\nnav:\n  - Home: index.md\n  - Guides:\n    - guides/setup.md\n    - VPN: guides/vpn.md\n",
        );
        assert_eq!(config.site_name.as_deref(), Some("IT Handbook"));
        assert_eq!(config.docs_dir, "content");
        let nav = config.nav.unwrap();
        assert_eq!(nav[0].path.as_deref(), Some("index.md"));
        assert_eq!(nav[1].children.len(), 2);
        assert_eq!(nav[1].children[1].title.as_deref(), Some("VPN"));
    }
}
