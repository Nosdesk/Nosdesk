//! Reading a zip an admin uploaded, without trusting it.
//!
//! Shared by the documentation import and the workspace archive import, so
//! neither caller parses a hostile zip on its own. Two halves:
//!
//! - [`spool`]: a multipart field streamed to a local temp file with a hard
//!   byte cap enforced while it streams, then copied to workspace storage
//!   (and back to a temp file later) without buffering the whole file.
//! - [`reader::SafeArchive`]: opens the zip from any `Read + Seek` (a temp
//!   file, or the bytes of an unsealed workspace export) and exposes its
//!   entries as validated lookup keys with bounded readers.
//!
//! Archive paths are only ever lookup keys. Nothing is written to disk under
//! an archive name: the one thing written is a nested part, to a temp file
//! with a random name.
//!
//! Each caller picks its own [`ArchiveOptions`]: limits, which names are
//! ignored, and whether a zip of zips (a split export) is opened as one.

pub mod paths;
pub mod reader;
pub mod spool;
pub mod text;

use std::path::PathBuf;

pub use paths::IgnoreRules;
pub use reader::{Entry, SafeArchive, SkipReason, Skipped};
pub use text::{decode_text, Text, TextEncoding};

use crate::services::imports::refusal::Refused;

const MIB: u64 = 1024 * 1024;

/// The bounds an archive is read under. Sizes are uncompressed bytes,
/// counted as they are read; headers only ever cause an earlier refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Entries in the archive, nested parts' entries included.
    pub max_entries: usize,
    /// One entry's uncompressed size. A read may ask for less.
    pub max_entry_bytes: u64,
    /// Everything read from the archive, nested parts included.
    pub max_total_bytes: u64,
    /// Uncompressed to compressed size, per entry.
    pub max_ratio: u64,
    /// Entries up to this size are never refused for their ratio: a small
    /// file of blank lines deflates past 100:1 and is no threat.
    pub ratio_floor_bytes: u64,
    /// Path segments in one name.
    pub max_depth: usize,
    /// Bytes in one path segment.
    pub max_segment_bytes: usize,
}

impl Default for ArchiveLimits {
    /// The documentation import's limits: 10,000 entries, 10 MB per entry
    /// (the collab image cap), 1 GiB in all, 100:1.
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_entry_bytes: 10 * MIB,
            max_total_bytes: 1024 * MIB,
            max_ratio: 100,
            ratio_floor_bytes: 64 * 1024,
            max_depth: 20,
            max_segment_bytes: 255,
        }
    }
}

/// Whether a zip of zips is opened as one archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedZips {
    /// Inner zips are ordinary entries.
    Never,
    /// When the outer zip holds nothing but zips (after ignored names,
    /// folders and symlinks are set aside), each inner zip is opened under
    /// the same rules and their entries merged into one key space: Notion
    /// and others split a large export into parts. One level only; a zip
    /// inside a part is an ordinary entry. An outer zip that holds anything
    /// else keeps its zips as ordinary entries.
    SplitExport,
}

/// How a caller opens an archive.
#[derive(Debug, Clone)]
pub struct ArchiveOptions {
    pub limits: ArchiveLimits,
    pub ignore: IgnoreRules,
    pub nested: NestedZips,
    /// Where nested parts are spooled. `None` is the system temp dir.
    pub temp_dir: Option<PathBuf>,
}

impl ArchiveOptions {
    /// The documentation import: its limits, the source tools' clutter
    /// ignored, split exports opened as one.
    pub fn documentation() -> Self {
        Self {
            limits: ArchiveLimits::default(),
            ignore: IgnoreRules::documentation_sources(),
            nested: NestedZips::SplitExport,
            temp_dir: None,
        }
    }

    /// A single zip written by a known producer (a workspace export): OS
    /// clutter ignored, nested zips left alone. The caller sets the limits
    /// its archives need.
    pub fn single(limits: ArchiveLimits) -> Self {
        Self {
            limits,
            ignore: IgnoreRules::os_metadata(),
            nested: NestedZips::Never,
            temp_dir: None,
        }
    }
}

/// Why an archive, or one of its entries, can't be read.
///
/// The archive itself is refused at open for [`NotAZip`](Self::NotAZip),
/// [`TooManyEntries`](Self::TooManyEntries), [`Encrypted`](Self::Encrypted),
/// [`UnsafePath`](Self::UnsafePath), [`Duplicate`](Self::Duplicate) and a
/// declared [`Ratio`](Self::Ratio). The rest come from one read: the entry
/// is the caller's to report, and the archive stays open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// Not a zip, or one too damaged to list. `part` names the nested part.
    NotAZip {
        part: Option<String>,
    },
    TooManyEntries {
        max: usize,
    },
    /// A password-protected entry. Nothing in it can be read.
    Encrypted {
        name: String,
    },
    /// A name that could reach outside a folder or isn't a usable path.
    UnsafePath {
        name: String,
        reason: &'static str,
    },
    /// Two entries with one name, or one key after normalising.
    Duplicate {
        name: String,
    },
    /// An entry that inflates more than the ratio allows.
    Ratio {
        name: String,
        max: u64,
    },
    /// An entry larger than the read allows.
    EntryTooLarge {
        name: String,
        max: u64,
    },
    /// The archive's total budget is spent.
    TotalTooLarge {
        max: u64,
    },
    /// No entry under this key.
    NotFound {
        name: String,
    },
    /// The entry's data is damaged (bad deflate stream, checksum mismatch).
    Corrupt {
        name: String,
        detail: String,
    },
    /// The local temp file or the source reader failed.
    Io(String),
}

impl ArchiveError {
    /// The machine-readable code a client words the refusal by.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotAZip { .. } => "archive_unreadable",
            Self::TooManyEntries { .. } => "archive_too_many_entries",
            Self::Encrypted { .. } => "archive_encrypted",
            Self::UnsafePath { .. } => "archive_unsafe_path",
            Self::Duplicate { .. } => "archive_duplicate_entry",
            Self::Ratio { .. } => "archive_compression_ratio",
            Self::EntryTooLarge { .. } => "archive_entry_too_large",
            Self::TotalTooLarge { .. } => "archive_too_large",
            Self::NotFound { .. } => "archive_entry_missing",
            Self::Corrupt { .. } => "archive_entry_corrupt",
            Self::Io(_) => "archive_io",
        }
    }
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAZip { part: None } => f.write_str("This file isn't a zip we can read."),
            Self::NotAZip { part: Some(p) } => {
                write!(f, "The zip inside it, {p}, isn't a zip we can read.")
            }
            Self::TooManyEntries { max } => {
                write!(f, "The zip holds more than {max} files.")
            }
            Self::Encrypted { .. } => {
                f.write_str("This zip is password-protected. Export it again without a password.")
            }
            Self::UnsafePath { name, reason } => {
                write!(
                    f,
                    "The zip has a file name we can't accept ({reason}): {name}"
                )
            }
            Self::Duplicate { name } => write!(f, "The zip holds {name} more than once."),
            Self::Ratio { name, max } => write!(
                f,
                "{name} unpacks to more than {max} times its packed size, so it isn't read."
            ),
            Self::EntryTooLarge { name, max } => {
                write!(f, "{name} is larger than {max} bytes.")
            }
            Self::TotalTooLarge { max } => {
                write!(f, "The zip unpacks to more than {max} bytes.")
            }
            Self::NotFound { name } => write!(f, "The zip has no file {name}."),
            Self::Corrupt { name, .. } => write!(f, "{name} is damaged in the zip."),
            Self::Io(e) => write!(f, "The upload couldn't be read: {e}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<ArchiveError> for Refused {
    fn from(e: ArchiveError) -> Self {
        let message = e.to_string();
        let refused = Refused::new(e.code(), message);
        match e {
            ArchiveError::NotAZip { part: Some(p) } => refused.arg("part", p),
            ArchiveError::TooManyEntries { max } => refused.arg("max", max),
            ArchiveError::UnsafePath { name, .. }
            | ArchiveError::Duplicate { name }
            | ArchiveError::NotFound { name }
            | ArchiveError::Corrupt { name, .. }
            | ArchiveError::Encrypted { name } => refused.arg("name", name),
            ArchiveError::Ratio { name, max } | ArchiveError::EntryTooLarge { name, max } => {
                refused.arg("name", name).arg("max", max)
            }
            ArchiveError::TotalTooLarge { max } => refused.arg("max", max),
            ArchiveError::NotAZip { part: None } | ArchiveError::Io(_) => refused,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_the_code_and_its_values() {
        let r: Refused = ArchiveError::EntryTooLarge {
            name: "a.md".into(),
            max: 2,
        }
        .into();
        assert_eq!(r.code, "archive_entry_too_large");
        assert_eq!(r.args["name"], "a.md");
        assert_eq!(r.args["max"], 2);
    }
}
