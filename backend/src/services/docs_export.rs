//! The documentation export (format 2): every page the caller may read, as
//! Markdown files in a zip an import reads back unchanged.
//!
//! - `nosdesk-docs.json`, the manifest: the workspace, the collections and
//!   every page with its place in the tree, order, status and icon.
//! - `{collection-slug}/{page-slug}.md` for a page at the top of a
//!   collection, `{page-slug}.md` for one in none, and a page's children in
//!   `{page-slug}/` beside its file. Paths never collide: a taken name gets
//!   `-2`, `-3`, compared without case so the zip unpacks on any system.
//! - Each file opens with front matter (title, uuid, slug, status, icon),
//!   then the body as `utils::markdown_export` writes it, with links to
//!   other exported pages made relative.
//! - `_assets/{page-uuid}/` holds the images the bodies show, read through
//!   workspace storage. Only images stored under an exported page are
//!   copied; any other source is left as it was.
//!
//! [`build`] does the database part inside the caller's connection;
//! [`write_zip`] then streams the zip into a temporary file, one image in
//! memory at a time.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::db::DbConnection;
use crate::models::{DocumentationCollection, DocumentationPage, DocumentationStatus};
use crate::repository::{self, PageAudience};
use crate::utils::markdown_export::{self, Links};
use crate::utils::storage::Storage;

pub const FORMAT: &str = "nosdesk-docs";
pub const FORMAT_VERSION: u32 = 2;
pub const MANIFEST_PATH: &str = "nosdesk-docs.json";
pub const ASSETS_DIR: &str = "_assets";

/// Where the editor's images are served from, and stored under.
const IMAGE_URL_PREFIX: &str = "/api/files/collab/doc/";
const IMAGE_KEY_PREFIX: &str = "collab/doc/";

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub format: &'static str,
    pub format_version: u32,
    pub workspace_uuid: Uuid,
    pub exported_at: String,
    pub collections: Vec<CollectionEntry>,
    pub pages: Vec<PageEntry>,
    /// Images a page shows that storage no longer holds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_assets: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CollectionEntry {
    pub uuid: Uuid,
    pub name: String,
    pub slug: String,
    /// The folder its top-level pages are in.
    pub path: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub order: i32,
}

#[derive(Debug, Serialize)]
pub struct PageEntry {
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub path: String,
    /// The parent page; none when the page is at the top, or its parent
    /// isn't in the export.
    pub parent: Option<Uuid>,
    /// The first collection the page is in, in collection order.
    pub collection: Option<Uuid>,
    pub order: Option<i32>,
    pub status: DocumentationStatus,
    pub icon: Option<String>,
}

/// An image to copy: its storage key and its path in the zip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub key: String,
    pub path: String,
}

/// Everything the zip holds, the images still to be read.
pub struct Export {
    pub manifest: Manifest,
    /// `(path, Markdown)`, one per page.
    pub files: Vec<(String, String)>,
    pub assets: Vec<Asset>,
}

/// Read and lay out every page `audience` may read, with the same filter the
/// JSON export applies, and write each page's Markdown.
pub fn build(
    conn: &mut DbConnection,
    audience: &PageAudience,
    workspace_uuid: Uuid,
) -> diesel::QueryResult<Export> {
    let pages = repository::get_documentation_pages(conn)?;
    let pages = audience.filter_pages(conn, pages)?;
    let collections = repository::documentation_collections::get_all_collections(conn)?;
    let collection_ids: Vec<i32> = collections.iter().map(|c| c.id).collect();
    let (_, hidden) = audience.hidden(conn, &[], &collection_ids)?;
    let collections: Vec<DocumentationCollection> = collections
        .into_iter()
        .filter(|c| !hidden.contains(&c.id))
        .collect();
    let memberships = repository::documentation_collections::get_all_page_memberships(conn)?;

    let places: Vec<PagePlace> = pages.iter().map(PagePlace::from).collect();
    let collection_slugs: Vec<(i32, String)> =
        collections.iter().map(|c| (c.id, c.slug.clone())).collect();
    let layout = Layout::new(&places, &collection_slugs, &memberships);
    let exported_uuids: HashSet<Uuid> = pages.iter().map(|p| p.uuid).collect();
    let mut by_reference: HashMap<String, String> = HashMap::new();
    for page in &pages {
        let path = &layout.paths[&page.id];
        by_reference.insert(page.id.to_string(), path.clone());
        by_reference.insert(page.slug.clone(), path.clone());
    }

    let collection_uuid: HashMap<i32, Uuid> = collections.iter().map(|c| (c.id, c.uuid)).collect();
    let page_uuid: HashMap<i32, Uuid> = pages.iter().map(|p| (p.id, p.uuid)).collect();
    let mut assets: BTreeMap<String, String> = BTreeMap::new();
    let mut files = Vec::with_capacity(pages.len());
    let mut entries = Vec::with_capacity(pages.len());
    for page in layout
        .order
        .iter()
        .filter_map(|id| pages.iter().find(|p| p.id == *id))
    {
        let path = layout.paths[&page.id].clone();
        let links = PageLinks {
            dir: parent_dir(&path).to_string(),
            pages: &by_reference,
            exported: &exported_uuids,
            assets: RefCell::new(BTreeMap::new()),
        };
        let body = crate::handlers::documentation::resolve_yjs_document(page, conn)
            .and_then(|bytes| markdown_export::yjs_to_markdown_for_export(&bytes, conn, &links))
            .unwrap_or_default();
        assets.append(&mut links.assets.into_inner());
        files.push((path.clone(), page_file(page, &body)));
        entries.push(PageEntry {
            uuid: page.uuid,
            title: page.title.clone(),
            slug: page.slug.clone(),
            path,
            parent: layout.parents[&page.id].and_then(|id| page_uuid.get(&id).copied()),
            collection: layout.collections[&page.id]
                .and_then(|id| collection_uuid.get(&id).copied()),
            order: page.display_order,
            status: page.status,
            icon: page.icon.clone(),
        });
    }

    let manifest = Manifest {
        format: FORMAT,
        format_version: FORMAT_VERSION,
        workspace_uuid,
        exported_at: chrono::Utc::now().to_rfc3339(),
        collections: collections
            .iter()
            .map(|c| CollectionEntry {
                uuid: c.uuid,
                name: c.name.clone(),
                slug: c.slug.clone(),
                path: layout.collection_dirs[&c.id].clone(),
                icon: c.icon.clone(),
                color: c.color.clone(),
                description: c.description.clone(),
                order: c.display_order,
            })
            .collect(),
        pages: entries,
        missing_assets: Vec::new(),
    };
    Ok(Export {
        manifest,
        files,
        assets: assets
            .into_iter()
            .map(|(key, path)| Asset { key, path })
            .collect(),
    })
}

/// Write the export to an unnamed temporary file, reading the images from
/// `storage` one at a time, and return it rewound.
pub async fn write_zip(
    export: Export,
    storage: Arc<dyn Storage>,
) -> std::io::Result<std::fs::File> {
    let Export {
        mut manifest,
        files,
        assets,
    } = export;
    let text = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);

    let mut zip = blocking(move || {
        let mut zip = ZipWriter::new(tempfile::tempfile()?);
        for (path, markdown) in files {
            zip.start_file(path, text).map_err(std::io::Error::other)?;
            zip.write_all(markdown.as_bytes())?;
        }
        Ok(zip)
    })
    .await?;

    for asset in assets {
        match storage.get_file(&asset.key).await {
            Ok(bytes) => {
                zip = blocking(move || {
                    zip.start_file(asset.path, stored)
                        .map_err(std::io::Error::other)?;
                    zip.write_all(&bytes)?;
                    Ok(zip)
                })
                .await?;
            }
            Err(_) => manifest.missing_assets.push(asset.path),
        }
    }

    blocking(move || {
        let json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        zip.start_file(MANIFEST_PATH, text)
            .map_err(std::io::Error::other)?;
        zip.write_all(&json)?;
        let mut file = zip.finish().map_err(std::io::Error::other)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    })
    .await
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(std::io::Error::other)?
}

/// A page's file: front matter, then its body.
fn page_file(page: &DocumentationPage, body: &str) -> String {
    let mut out = front_matter(
        &page.title,
        page.uuid,
        &page.slug,
        page.status,
        page.icon.as_deref(),
    );
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    out
}

fn front_matter(
    title: &str,
    uuid: Uuid,
    slug: &str,
    status: DocumentationStatus,
    icon: Option<&str>,
) -> String {
    // A JSON string is a valid YAML double-quoted scalar.
    let quoted = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    let status = match status {
        DocumentationStatus::Draft => "draft",
        DocumentationStatus::Published => "published",
        DocumentationStatus::Archived => "archived",
        DocumentationStatus::Deleted => "deleted",
    };
    let mut out = format!(
        "---\ntitle: {}\nuuid: {uuid}\nslug: {}\nstatus: {status}\n",
        quoted(title),
        quoted(slug),
    );
    if let Some(icon) = icon {
        out.push_str(&format!("icon: {}\n", quoted(icon)));
    }
    out.push_str("---\n");
    out
}

// Layout.

/// What the layout needs of a page.
#[derive(Debug, Clone)]
pub struct PagePlace {
    pub id: i32,
    pub slug: String,
    pub title: String,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
}

impl From<&DocumentationPage> for PagePlace {
    fn from(page: &DocumentationPage) -> Self {
        PagePlace {
            id: page.id,
            slug: page.slug.clone(),
            title: page.title.clone(),
            parent_id: page.parent_id,
            display_order: page.display_order,
        }
    }
}

/// Where each page and collection goes in the zip.
#[derive(Debug, Default)]
pub struct Layout {
    /// Page id to its file's path.
    pub paths: HashMap<i32, String>,
    /// Collection id to its folder.
    pub collection_dirs: HashMap<i32, String>,
    /// Page id to its parent in the export.
    pub parents: HashMap<i32, Option<i32>>,
    /// Page id to its first collection in the export.
    pub collections: HashMap<i32, Option<i32>>,
    /// Page ids in the order their files are written: collections in order,
    /// each page before its children.
    pub order: Vec<i32>,
}

impl Layout {
    /// `collections` as `(id, slug)` in display order; `memberships` as
    /// `(collection_id, page_id)`.
    pub fn new(
        pages: &[PagePlace],
        collections: &[(i32, String)],
        memberships: &[(i32, i32)],
    ) -> Self {
        let mut layout = Layout::default();
        let mut names = Names::default();
        let exported: HashSet<i32> = pages.iter().map(|p| p.id).collect();

        let rank: HashMap<i32, usize> = collections
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        for page in pages {
            let first = memberships
                .iter()
                .filter(|(c, p)| *p == page.id && rank.contains_key(c))
                .min_by_key(|(c, _)| rank[c])
                .map(|(c, _)| *c);
            layout.collections.insert(page.id, first);
            layout
                .parents
                .insert(page.id, page.parent_id.filter(|p| exported.contains(p)));
        }

        for (id, slug) in collections {
            let dir = names.claim("", &file_stem(slug, "collection"));
            layout.collection_dirs.insert(*id, dir);
        }

        let mut children: HashMap<Option<i32>, Vec<&PagePlace>> = HashMap::new();
        for page in pages {
            children
                .entry(layout.parents[&page.id])
                .or_default()
                .push(page);
        }
        for siblings in children.values_mut() {
            siblings.sort_by_cached_key(|p| {
                (
                    p.display_order.is_none(),
                    p.display_order,
                    p.title.to_lowercase(),
                    p.id,
                )
            });
        }

        let roots: Vec<&PagePlace> = children.get(&None).cloned().unwrap_or_default();
        let mut placed = HashSet::new();
        let mut groups: Vec<(String, Vec<&PagePlace>)> = collections
            .iter()
            .map(|(id, _)| {
                let members = roots
                    .iter()
                    .copied()
                    .filter(|p| layout.collections[&p.id] == Some(*id))
                    .collect();
                (layout.collection_dirs[id].clone(), members)
            })
            .collect();
        let loose = roots
            .iter()
            .copied()
            .filter(|p| layout.collections[&p.id].is_none())
            .collect();
        groups.push((String::new(), loose));
        for (dir, members) in groups {
            for root in members {
                layout.place(root, &dir, &children, &mut names, &mut placed);
            }
        }
        // A page whose parents loop back to it is reachable from no root.
        for page in pages {
            if !placed.contains(&page.id) {
                layout.parents.insert(page.id, None);
                layout.place(page, "", &children, &mut names, &mut placed);
            }
        }
        layout
    }

    fn place(
        &mut self,
        page: &PagePlace,
        dir: &str,
        children: &HashMap<Option<i32>, Vec<&PagePlace>>,
        names: &mut Names,
        placed: &mut HashSet<i32>,
    ) {
        if !placed.insert(page.id) {
            return;
        }
        let stem = names.claim(dir, &file_stem(&page.slug, &page.title));
        let folder = join(dir, &stem);
        self.paths.insert(page.id, format!("{folder}.md"));
        self.order.push(page.id);
        for child in children.get(&Some(page.id)).into_iter().flatten() {
            self.place(child, &folder, children, names, placed);
        }
    }
}

/// The names taken in the zip, compared without case.
struct Names {
    taken: HashSet<String>,
}

impl Default for Names {
    fn default() -> Self {
        let taken = [MANIFEST_PATH, ASSETS_DIR, "_assets.md"]
            .into_iter()
            .map(str::to_string)
            .collect();
        Names { taken }
    }
}

impl Names {
    /// A free name in `dir` for both `{name}.md` and the folder `{name}/`.
    fn claim(&mut self, dir: &str, stem: &str) -> String {
        for n in 1usize.. {
            let name = if n == 1 {
                stem.to_string()
            } else {
                format!("{stem}-{n}")
            };
            let folder = join(dir, &name).to_lowercase();
            let file = format!("{folder}.md");
            if !self.taken.contains(&folder) && !self.taken.contains(&file) {
                self.taken.insert(folder);
                self.taken.insert(file);
                return name;
            }
        }
        unreachable!("an unbounded range always yields a free name")
    }
}

/// A file or folder name from a slug: letters, digits, `-` and `_`, at most
/// 80 characters, never empty and never a name Windows reserves.
fn file_stem(slug: &str, fallback: &str) -> String {
    let clean = |s: &str| -> String {
        let mut out = String::new();
        for c in s.chars() {
            if c.is_alphanumeric() || c == '_' {
                out.extend(c.to_lowercase());
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        let trimmed: String = out.trim_matches('-').chars().take(80).collect();
        trimmed.trim_end_matches('-').to_string()
    };
    let mut stem = clean(slug);
    if stem.is_empty() {
        stem = clean(fallback);
    }
    if stem.is_empty() {
        stem = "page".to_string();
    }
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        stem.push_str("-page");
    }
    stem
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// `to` relative to the folder `from_dir`.
fn relative(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let target: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count()
        .min(target.len().saturating_sub(1));
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&target[common..]);
    parts.join("/")
}

/// Rewrites a page's links to other exported pages as relative paths, and
/// its images stored under an exported page to their copy in `_assets/`.
struct PageLinks<'a> {
    dir: String,
    /// Page id and slug to the page's path.
    pages: &'a HashMap<String, String>,
    exported: &'a HashSet<Uuid>,
    /// Storage key to zip path, for the images this page shows.
    assets: RefCell<BTreeMap<String, String>>,
}

impl Links for PageLinks<'_> {
    fn href(&self, href: &str) -> String {
        let Some(rest) = href.strip_prefix("/documentation/") else {
            return href.to_string();
        };
        let end = rest.find(['?', '#']).unwrap_or(rest.len());
        let (reference, suffix) = rest.split_at(end);
        match self.pages.get(reference) {
            Some(path) if !reference.contains('/') => {
                let fragment = suffix.find('#').map(|i| &suffix[i..]).unwrap_or("");
                format!("{}{fragment}", relative(&self.dir, path))
            }
            _ => href.to_string(),
        }
    }

    fn image_src(&self, src: &str) -> String {
        let Some((uuid, name)) = src
            .strip_prefix(IMAGE_URL_PREFIX)
            .and_then(|rest| rest.split_once('/'))
        else {
            return src.to_string();
        };
        let Ok(uuid) = Uuid::parse_str(uuid) else {
            return src.to_string();
        };
        let safe_name = !name.is_empty()
            && name.len() <= 255
            && !name.starts_with('.')
            && !name.contains(['/', '\\', '\0', '?', '#']);
        if !safe_name || !self.exported.contains(&uuid) {
            return src.to_string();
        }
        let key = format!("{IMAGE_KEY_PREFIX}{uuid}/{name}");
        if !crate::utils::storage::is_safe_storage_path(&key) {
            return src.to_string();
        }
        let path = format!("{ASSETS_DIR}/{uuid}/{name}");
        self.assets.borrow_mut().insert(key, path.clone());
        relative(&self.dir, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        id: i32,
        title: &str,
        slug: &str,
        parent: Option<i32>,
        order: Option<i32>,
    ) -> PagePlace {
        PagePlace {
            id,
            slug: slug.to_string(),
            title: title.to_string(),
            parent_id: parent,
            display_order: order,
        }
    }

    #[test]
    fn pages_lay_out_by_collection_and_parent_without_collisions() {
        let pages = vec![
            page(1, "VPN", "vpn", None, Some(0)),
            page(2, "Setup", "setup", Some(1), Some(1)),
            page(3, "Setup", "SETUP", Some(1), Some(0)),
            page(4, "Loose", "loose", None, None),
            page(5, "IT", "it", None, Some(1)),
            page(6, "", "", None, Some(2)),
            // Its parent isn't in the export.
            page(7, "Orphan", "orphan", Some(99), None),
        ];
        let collections = vec![(10, "it".to_string())];
        let memberships = vec![(10, 1), (10, 5), (10, 6), (10, 2), (11, 4)];
        let layout = Layout::new(&pages, &collections, &memberships);
        let path = |id: i32| layout.paths[&id].as_str();
        assert_eq!(layout.collection_dirs[&10], "it");
        assert_eq!(path(1), "it/vpn.md");
        // Same-title siblings both get a file; the first in order keeps the
        // name and the other is numbered, whatever the case.
        assert_eq!(path(3), "it/vpn/setup.md");
        assert_eq!(path(2), "it/vpn/setup-2.md");
        assert_eq!(path(5), "it/it.md");
        assert_eq!(path(6), "it/page.md");
        // Not in an exported collection, and a root page named like the
        // collection's folder is numbered rather than merged into it.
        assert_eq!(path(4), "loose.md");
        assert_eq!(path(7), "orphan.md");
        assert_eq!(layout.parents[&7], None);
        assert_eq!(layout.parents[&2], Some(1));
        assert_eq!(layout.collections[&2], Some(10));
        assert_eq!(layout.collections[&4], None);
        assert_eq!(layout.order, vec![1, 3, 2, 5, 6, 4, 7]);
    }

    #[test]
    fn a_root_page_cannot_take_a_collections_folder() {
        let pages = vec![page(1, "IT", "it", None, None)];
        let collections = vec![(10, "it".to_string())];
        let layout = Layout::new(&pages, &collections, &[]);
        assert_eq!(layout.paths[&1], "it-2.md");
    }

    #[test]
    fn links_to_exported_pages_become_relative() {
        let pages: HashMap<String, String> = [
            ("12".to_string(), "it/vpn.md".to_string()),
            ("vpn".to_string(), "it/vpn.md".to_string()),
            ("wifi".to_string(), "loose.md".to_string()),
        ]
        .into();
        let page_uuid = Uuid::from_u128(7);
        let exported: HashSet<Uuid> = [page_uuid].into();
        let links = PageLinks {
            dir: "it/vpn".to_string(),
            pages: &pages,
            exported: &exported,
            assets: RefCell::new(BTreeMap::new()),
        };
        assert_eq!(links.href("/documentation/12"), "../vpn.md");
        assert_eq!(links.href("/documentation/vpn#setup"), "../vpn.md#setup");
        assert_eq!(links.href("/documentation/wifi?x=1"), "../../loose.md");
        assert_eq!(links.href("/documentation/gone"), "/documentation/gone");
        assert_eq!(links.href("https://example.com"), "https://example.com");

        let src = format!("/api/files/collab/doc/{page_uuid}/0190_a b.png");
        assert_eq!(
            links.image_src(&src),
            format!("../../_assets/{page_uuid}/0190_a b.png")
        );
        let other = format!("/api/files/collab/doc/{}/x.png", Uuid::from_u128(8));
        assert_eq!(
            links.image_src(&other),
            other,
            "not an exported page's image"
        );
        let sneaky = format!("/api/files/collab/doc/{page_uuid}/../x.png");
        assert_eq!(links.image_src(&sneaky), sneaky);
        assert_eq!(
            links.assets.into_inner(),
            BTreeMap::from([(
                format!("collab/doc/{page_uuid}/0190_a b.png"),
                format!("_assets/{page_uuid}/0190_a b.png")
            )])
        );
    }

    #[test]
    fn a_page_file_opens_with_front_matter() {
        let uuid = Uuid::from_u128(1);
        assert_eq!(
            front_matter("Say \"hi\"", uuid, "say-hi", DocumentationStatus::Draft, Some("👋")),
            format!(
                "---\ntitle: \"Say \\\"hi\\\"\"\nuuid: {uuid}\nslug: \"say-hi\"\nstatus: draft\nicon: \"👋\"\n---\n"
            )
        );
    }
}
