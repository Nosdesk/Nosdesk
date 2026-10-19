//! Turning an entry's raw name into a lookup key, and the names a caller
//! sets aside.
//!
//! A key is `/`-separated, relative, with no empty, `.` or `..` segment.
//! Anything that could name a place outside the archive's root is refused
//! rather than repaired: an export with such a name was not made by a tool
//! we read.

use super::ArchiveLimits;

/// Paths longer than this are refused whatever their depth.
const MAX_PATH_BYTES: usize = 4096;

/// A validated name: its key, and whether the entry is a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalisedName {
    pub key: String,
    pub is_dir: bool,
}

/// Normalise `name` (`\` read as `/`) and check it against the path rules.
/// `Err` holds the reason the name is refused.
pub fn normalise(name: &str, limits: &ArchiveLimits) -> Result<NormalisedName, &'static str> {
    if name.contains('\0') {
        return Err("NUL in name");
    }
    if name.len() > MAX_PATH_BYTES {
        return Err("path too long");
    }
    let unified = name.replace('\\', "/");
    if unified.starts_with('/') {
        return Err("absolute path");
    }
    if has_drive_letter(&unified) {
        return Err("drive letter");
    }
    let is_dir = unified.ends_with('/');
    let mut segments = Vec::new();
    for segment in unified.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err("parent folder segment"),
            s if s.len() > limits.max_segment_bytes => return Err("name too long"),
            s => segments.push(s),
        }
    }
    if segments.is_empty() {
        return Err("empty name");
    }
    if segments.len() > limits.max_depth {
        return Err("folders nested too deep");
    }
    Ok(NormalisedName {
        key: segments.join("/"),
        is_dir,
    })
}

/// `C:` or `c:/...`: a Windows drive, absolute or drive-relative.
fn has_drive_letter(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Names a caller sets aside. Matching is exact and case-sensitive, on
/// whole segments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IgnoreRules {
    /// Any segment starting with `.` (dot files and dot folders).
    pub dot_names: bool,
    /// Folder names ignored at any depth, with everything under them.
    pub folders: Vec<String>,
    /// File names ignored at any depth.
    pub files: Vec<String>,
}

impl IgnoreRules {
    /// Nothing is ignored.
    pub fn none() -> Self {
        Self::default()
    }

    /// What an OS adds when it zips a folder: macOS resource forks and
    /// Finder state, Windows thumbnails.
    pub fn os_metadata() -> Self {
        Self {
            dot_names: false,
            folders: vec!["__MACOSX".into()],
            files: vec![".DS_Store".into(), "Thumbs.db".into(), "desktop.ini".into()],
        }
    }

    /// The documentation import's list: dot names, OS clutter, tool state
    /// (`.git`, `.obsidian`, `.trash`, `node_modules`) and generated sites
    /// (`site`, `build`, `_site`).
    pub fn documentation_sources() -> Self {
        let mut rules = Self::os_metadata();
        rules.dot_names = true;
        rules.folders.extend(
            [
                ".git",
                ".obsidian",
                ".trash",
                "node_modules",
                "site",
                "build",
                "_site",
            ]
            .map(String::from),
        );
        rules
    }

    /// Whether the entry at `key` is set aside. `is_dir` says the last
    /// segment is a folder.
    pub fn ignores(&self, key: &str, is_dir: bool) -> bool {
        let segments: Vec<&str> = key.split('/').collect();
        let last = segments.len() - 1;
        segments.iter().enumerate().any(|(i, segment)| {
            let folder = i < last || is_dir;
            (self.dot_names && segment.starts_with('.'))
                || (folder && self.folders.iter().any(|f| f == segment))
                || (!folder && self.files.iter().any(|f| f == segment))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Result<String, &'static str> {
        normalise(name, &ArchiveLimits::default()).map(|n| n.key)
    }

    #[test]
    fn backslashes_dot_segments_and_doubled_slashes_normalise() {
        assert_eq!(key("Docs\\Setup\\vpn.md"), Ok("Docs/Setup/vpn.md".into()));
        assert_eq!(key("./a//b/./c.md"), Ok("a/b/c.md".into()));
        let dir = normalise("Docs/Setup/", &ArchiveLimits::default()).unwrap();
        assert_eq!(dir.key, "Docs/Setup");
        assert!(dir.is_dir);
    }

    #[test]
    fn names_that_could_leave_the_root_are_refused() {
        assert_eq!(key("../etc/passwd"), Err("parent folder segment"));
        assert_eq!(key("a/../../b"), Err("parent folder segment"));
        assert_eq!(key("a\\..\\b"), Err("parent folder segment"));
        assert_eq!(key("/etc/passwd"), Err("absolute path"));
        assert_eq!(key("\\\\server\\share\\x"), Err("absolute path"));
        assert_eq!(key("C:\\Windows\\x.md"), Err("drive letter"));
        assert_eq!(key("c:x.md"), Err("drive letter"));
        assert_eq!(key("a\0b.md"), Err("NUL in name"));
        assert_eq!(key("./"), Err("empty name"));
    }

    #[test]
    fn long_names_and_deep_folders_are_refused() {
        let long = "a".repeat(256);
        assert_eq!(key(&format!("{long}.md")), Err("name too long"));
        assert!(key(&format!("{}.md", "a".repeat(250))).is_ok());
        let deep = vec!["d"; 20].join("/") + "/x.md";
        assert_eq!(key(&deep), Err("folders nested too deep"));
        let ok = vec!["d"; 19].join("/") + "/x.md";
        assert!(key(&ok).is_ok());
    }

    #[test]
    fn documentation_rules_ignore_tool_folders_and_dot_names() {
        let rules = IgnoreRules::documentation_sources();
        for ignored in [
            "__MACOSX/._a.md",
            ".git/config",
            "vault/.obsidian/app.json",
            "node_modules/x/readme.md",
            "site/index.html",
            "docs/build/a.md",
            "_site/a.md",
            ".DS_Store",
            "notes/.hidden.md",
        ] {
            assert!(rules.ignores(ignored, false), "{ignored}");
        }
        assert!(rules.ignores("site", true));
        assert!(
            !rules.ignores("docs/site.md", false),
            "a file named like a folder"
        );
        assert!(!rules.ignores("docs/build", false));
        assert!(!rules.ignores("docs/setup/vpn.md", false));
    }

    #[test]
    fn os_metadata_rules_keep_dot_names_and_site_folders() {
        let rules = IgnoreRules::os_metadata();
        assert!(rules.ignores("__MACOSX/files/a", false));
        assert!(rules.ignores("files/.DS_Store", false));
        assert!(!rules.ignores("files/.gitbook/a.png", false));
        assert!(!rules.ignores("site/index.html", false));
        assert!(!IgnoreRules::none().ignores("__MACOSX/a", false));
    }
}
