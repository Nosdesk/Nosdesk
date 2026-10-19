//! [`SafeArchive`]: an uploaded zip's entries as validated keys with
//! bounded readers.
//!
//! At open, every name is checked ([`super::paths`]), folders, ignored names
//! and symlinks are set aside, and the archive is refused for an encrypted
//! entry, an unsafe or duplicate name, too many entries, or a header that
//! declares a bomb's ratio. Nothing is decompressed at open except the
//! parts of a split export.
//!
//! Every read goes through `Read::take` with the smallest of the read's own
//! cap, the per-entry cap, the ratio bound and what is left of the total, so
//! a header that lies about sizes gains nothing.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};

use tempfile::NamedTempFile;
use zip::ZipArchive;

use super::paths::normalise;
use super::text::{decode_text, Text};
use super::{ArchiveError, ArchiveLimits, ArchiveOptions, NestedZips};

/// What a zip is read from: a temp file, or bytes already in memory.
pub trait Source: Read + Seek + Send {}
impl<T: Read + Seek + Send> Source for T {}

type Zip = ZipArchive<Box<dyn Source>>;

/// One readable entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The normalised path: the only name callers see.
    pub key: String,
    /// Uncompressed size as the header declares it. Reads enforce the real
    /// size; this is for planning (skipping a file too big to use) only.
    pub declared_size: u64,
    /// The raw name wasn't UTF-8 and was read as CP437, the zip default, so
    /// its characters may be wrong.
    pub name_guessed: bool,
    /// The nested part it came from, for a split export.
    pub part: Option<String>,
    compressed_size: u64,
    location: (usize, usize),
}

/// An entry set aside at open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub key: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Matched the caller's [`super::IgnoreRules`]. Kept so a caller can
    /// still tell a source by its tool folders (`.obsidian/`).
    Ignored,
    /// A symbolic link: never followed, never read.
    Symlink,
}

struct Part {
    zip: Zip,
    /// The spooled bytes of a nested part, removed when the archive drops.
    _spool: Option<NamedTempFile>,
}

#[derive(Default)]
struct Listing {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    folders: Vec<String>,
    skipped: Vec<Skipped>,
    counted: usize,
}

/// An opened archive. Reading is synchronous: run it in `web::block` or a
/// blocking task, with no database connection held.
pub struct SafeArchive {
    parts: Vec<Part>,
    listing: Listing,
    limits: ArchiveLimits,
    split: Vec<String>,
    read_total: u64,
}

impl std::fmt::Debug for SafeArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeArchive")
            .field("entries", &self.listing.entries.len())
            .field("parts", &self.split)
            .field("read_total", &self.read_total)
            .finish()
    }
}

impl SafeArchive {
    /// Open the zip in `reader` under `options`.
    pub fn open<R: Read + Seek + Send + 'static>(
        reader: R,
        options: &ArchiveOptions,
    ) -> Result<Self, ArchiveError> {
        let outer = open_zip(Box::new(reader), None, &options.limits, 0)?;
        let mut archive = Self {
            parts: vec![Part {
                zip: outer,
                _spool: None,
            }],
            listing: Listing::default(),
            limits: options.limits.clone(),
            split: Vec::new(),
            read_total: 0,
        };
        list_part(
            &mut archive.parts[0].zip,
            0,
            None,
            options,
            &mut archive.listing,
        )?;
        let entries = &archive.listing.entries;
        let only_zips = !entries.is_empty()
            && entries
                .iter()
                .all(|e| e.key.to_ascii_lowercase().ends_with(".zip"));
        if options.nested == NestedZips::SplitExport && only_zips {
            archive.open_parts(options)?;
        }
        Ok(archive)
    }

    /// Open each of the outer zip's entries as a part under the same rules,
    /// listed into one key space. The outer zip's entries stop being
    /// readable; its handle stays, as the parts' locations index past it.
    fn open_parts(&mut self, options: &ArchiveOptions) -> Result<(), ArchiveError> {
        let outer_entries = std::mem::take(&mut self.listing.entries);
        self.listing.index.clear();
        let single = ArchiveOptions {
            nested: NestedZips::Never,
            ..options.clone()
        };
        for entry in &outer_entries {
            let spool = self.spool_part(entry, options)?;
            let file = spool.reopen().map_err(io_error)?;
            let mut zip = open_zip(
                Box::new(file),
                Some(&entry.key),
                &self.limits,
                self.listing.counted,
            )?;
            let part_no = self.parts.len();
            list_part(
                &mut zip,
                part_no,
                Some(&entry.key),
                &single,
                &mut self.listing,
            )?;
            self.parts.push(Part {
                zip,
                _spool: Some(spool),
            });
            self.split.push(entry.key.clone());
        }
        Ok(())
    }

    /// Copy a nested part to a temp file, bounded by its ratio and the total
    /// budget. The per-entry cap doesn't apply: a part is a whole export.
    fn spool_part(
        &mut self,
        entry: &Entry,
        options: &ArchiveOptions,
    ) -> Result<NamedTempFile, ArchiveError> {
        let mut spool = match &options.temp_dir {
            Some(dir) => NamedTempFile::new_in(dir),
            None => NamedTempFile::new(),
        }
        .map_err(io_error)?;
        self.copy_bounded(entry.clone(), u64::MAX, false, spool.as_file_mut())?;
        spool.as_file_mut().flush().map_err(io_error)?;
        Ok(spool)
    }

    /// Readable entries, in archive order.
    pub fn entries(&self) -> &[Entry] {
        &self.listing.entries
    }

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.listing
            .index
            .get(key)
            .map(|&i| &self.listing.entries[i])
    }

    /// Folder entries, as keys. Folders are also implied by entry keys;
    /// these are the ones the zip lists explicitly, empty ones included.
    pub fn folders(&self) -> &[String] {
        &self.listing.folders
    }

    /// Entries set aside at open: ignored names and symlinks.
    pub fn skipped(&self) -> &[Skipped] {
        &self.listing.skipped
    }

    /// The nested parts' names when this was a split export.
    pub fn split_parts(&self) -> &[String] {
        &self.split
    }

    /// Uncompressed bytes read so far, nested parts included.
    pub fn bytes_read(&self) -> u64 {
        self.read_total
    }

    /// The entry at `key`, at most `max` bytes (and never more than the
    /// per-entry cap).
    pub fn read(&mut self, key: &str, max: u64) -> Result<Vec<u8>, ArchiveError> {
        let entry = self.lookup(key)?;
        // The header only sizes the first allocation, and only up to 1 MiB.
        let mut out = Vec::with_capacity(entry.declared_size.min(max).min(1 << 20) as usize);
        self.copy_bounded(entry, max, true, &mut out)?;
        Ok(out)
    }

    /// The entry at `key` decoded as text ([`decode_text`]).
    pub fn read_text(&mut self, key: &str, max: u64) -> Result<Text, ArchiveError> {
        Ok(decode_text(&self.read(key, max)?))
    }

    /// Stream the entry at `key` into `out`, at most `max` bytes. Returns
    /// the bytes written. On an error `out` may hold a partial copy.
    pub fn copy_to(
        &mut self,
        key: &str,
        max: u64,
        out: &mut impl Write,
    ) -> Result<u64, ArchiveError> {
        let entry = self.lookup(key)?;
        self.copy_bounded(entry, max, true, out)
    }

    fn lookup(&self, key: &str) -> Result<Entry, ArchiveError> {
        self.entry(key)
            .cloned()
            .ok_or_else(|| ArchiveError::NotFound {
                name: key.to_string(),
            })
    }

    /// The most `entry` may produce, and the error for going past it.
    fn allowance(&self, entry: &Entry, max: u64, entry_cap: bool) -> (u64, ArchiveError) {
        let limits = &self.limits;
        let cap = if entry_cap {
            max.min(limits.max_entry_bytes)
        } else {
            max
        };
        let mut best = (
            cap,
            ArchiveError::EntryTooLarge {
                name: entry.key.clone(),
                max: cap,
            },
        );
        let ratio = entry
            .compressed_size
            .saturating_mul(limits.max_ratio)
            .max(limits.ratio_floor_bytes);
        if ratio < best.0 {
            best = (
                ratio,
                ArchiveError::Ratio {
                    name: entry.key.clone(),
                    max: limits.max_ratio,
                },
            );
        }
        let left = limits.max_total_bytes.saturating_sub(self.read_total);
        if left < best.0 {
            best = (
                left,
                ArchiveError::TotalTooLarge {
                    max: limits.max_total_bytes,
                },
            );
        }
        best
    }

    fn copy_bounded(
        &mut self,
        entry: Entry,
        max: u64,
        entry_cap: bool,
        out: &mut impl Write,
    ) -> Result<u64, ArchiveError> {
        let (allowed, over) = self.allowance(&entry, max, entry_cap);
        // A header that admits to more is refused before anything inflates.
        if entry.declared_size > allowed {
            return Err(over);
        }
        let (part, index) = entry.location;
        let corrupt = |e: &dyn std::fmt::Display| ArchiveError::Corrupt {
            name: entry.key.clone(),
            detail: e.to_string(),
        };
        let file = self.parts[part]
            .zip
            .by_index(index)
            .map_err(|e| corrupt(&e))?;
        let (produced, outcome) = copy_limited(file, allowed, out);
        self.read_total += produced;
        match outcome {
            Ok(()) => Ok(produced),
            Err(Overrun::Over) => Err(over),
            Err(Overrun::Read(e)) => Err(corrupt(&e)),
            Err(Overrun::Write(e)) => Err(io_error(e)),
        }
    }
}

enum Overrun {
    /// The source produced more than allowed.
    Over,
    Read(io::Error),
    Write(io::Error),
}

/// Copy at most `allowed` bytes of `source` into `out` through
/// `Read::take`, asking for one byte more so a source that goes over is
/// caught without reading any further. Returns the bytes read either way.
fn copy_limited(
    source: impl Read,
    allowed: u64,
    out: &mut impl Write,
) -> (u64, Result<(), Overrun>) {
    let mut bounded = source.take(allowed.saturating_add(1));
    let mut buf = vec![0u8; 64 * 1024];
    let mut produced: u64 = 0;
    loop {
        let n = match bounded.read(&mut buf) {
            Ok(0) => return (produced, Ok(())),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return (produced, Err(Overrun::Read(e))),
        };
        produced += n as u64;
        if produced > allowed {
            return (produced, Err(Overrun::Over));
        }
        if let Err(e) = out.write_all(&buf[..n]) {
            return (produced, Err(Overrun::Write(e)));
        }
    }
}

fn io_error(e: io::Error) -> ArchiveError {
    ArchiveError::Io(e.to_string())
}

/// Open one zip, refusing one that declares more entries than are left
/// before the central directory is parsed, and one whose names collide.
fn open_zip(
    mut src: Box<dyn Source>,
    part: Option<&str>,
    limits: &ArchiveLimits,
    counted: usize,
) -> Result<Zip, ArchiveError> {
    let not_a_zip = || ArchiveError::NotAZip {
        part: part.map(str::to_string),
    };
    let too_many = ArchiveError::TooManyEntries {
        max: limits.max_entries,
    };
    let left = limits.max_entries.saturating_sub(counted) as u64;
    let declared = declared_entries(&mut src).map_err(io_error)?;
    if declared.is_some_and(|n| n > left) {
        return Err(too_many);
    }
    src.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let zip = ZipArchive::new(src).map_err(|_| not_a_zip())?;
    if zip.len() as u64 > left {
        return Err(too_many);
    }
    // The zip crate keeps one entry per raw name, so a name listed twice
    // shows as fewer entries than the directory declares.
    if declared.is_some_and(|n| n != zip.len() as u64) {
        return Err(ArchiveError::Duplicate {
            name: part.unwrap_or("an entry").to_string(),
        });
    }
    Ok(zip)
}

/// The entry count the end-of-central-directory record declares (ZIP64
/// included), or `None` when the record can't be found exactly, in which
/// case the count is checked after the zip crate has listed it.
fn declared_entries(src: &mut Box<dyn Source>) -> io::Result<Option<u64>> {
    const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const LOCATOR: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
    const EOCD64: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
    let len = src.seek(SeekFrom::End(0))?;
    let tail_len = len.min(22 + 65_535);
    src.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; tail_len as usize];
    src.read_exact(&mut tail)?;
    if tail.len() < 22 {
        return Ok(None);
    }
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
    let u64_at = |b: &[u8], at: usize| {
        let mut x = [0u8; 8];
        x.copy_from_slice(&b[at..at + 8]);
        u64::from_le_bytes(x)
    };
    for pos in (0..=tail.len() - 22).rev() {
        if tail[pos..pos + 4] != EOCD {
            continue;
        }
        // Only the record whose comment runs exactly to the end.
        if pos + 22 + usize::from(u16_at(&tail, pos + 20)) != tail.len() {
            continue;
        }
        let total = u16_at(&tail, pos + 10);
        if total != 0xFFFF {
            return Ok(Some(u64::from(total)));
        }
        if pos < 20 || tail[pos - 20..pos - 16] != LOCATOR {
            return Ok(None);
        }
        let record = u64_at(&tail, pos - 20 + 8);
        let mut eocd64 = [0u8; 40];
        if src.seek(SeekFrom::Start(record)).is_err() || src.read_exact(&mut eocd64).is_err() {
            return Ok(None);
        }
        if eocd64[..4] != EOCD64 {
            return Ok(None);
        }
        return Ok(Some(u64_at(&eocd64, 32)));
    }
    Ok(None)
}

/// Check and list every entry of one zip into `listing`.
fn list_part(
    zip: &mut Zip,
    part_no: usize,
    part: Option<&str>,
    options: &ArchiveOptions,
    listing: &mut Listing,
) -> Result<(), ArchiveError> {
    let limits = &options.limits;
    listing.counted += zip.len();
    if listing.counted > limits.max_entries {
        return Err(ArchiveError::TooManyEntries {
            max: limits.max_entries,
        });
    }
    for i in 0..zip.len() {
        let data = zip.by_index_data(i).map_err(|_| ArchiveError::NotAZip {
            part: part.map(str::to_string),
        })?;
        let raw = data.name_raw();
        let name_guessed = std::str::from_utf8(raw).is_err();
        let name = if raw.contains(&0) {
            String::from_utf8_lossy(raw).into_owned()
        } else {
            data.name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned())
        };
        let unsafe_path = |reason| ArchiveError::UnsafePath {
            name: name.clone(),
            reason,
        };
        if raw.contains(&0) {
            return Err(unsafe_path("NUL in name"));
        }
        let normalised = normalise(&name, limits).map_err(unsafe_path)?;
        let key = normalised.key;
        let is_dir = normalised.is_dir || data.is_dir();

        if options.ignore.ignores(&key, is_dir) {
            listing.skipped.push(Skipped {
                key,
                reason: SkipReason::Ignored,
            });
            continue;
        }
        if is_dir {
            listing.folders.push(key);
            continue;
        }
        if data.is_symlink() {
            listing.skipped.push(Skipped {
                key,
                reason: SkipReason::Symlink,
            });
            continue;
        }
        if data.encrypted() {
            return Err(ArchiveError::Encrypted { name: key });
        }
        let size = data.size();
        let compressed = data.compressed_size();
        if size > limits.ratio_floor_bytes && size / compressed.max(1) > limits.max_ratio {
            return Err(ArchiveError::Ratio {
                name: key,
                max: limits.max_ratio,
            });
        }
        if listing.index.contains_key(&key) {
            return Err(ArchiveError::Duplicate { name: key });
        }
        listing.index.insert(key.clone(), listing.entries.len());
        listing.entries.push(Entry {
            key,
            declared_size: size,
            name_guessed,
            part: part.map(str::to_string),
            compressed_size: compressed,
            location: (part_no, i),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use zip::write::SimpleFileOptions;
    use zip::{AesMode, CompressionMethod, ZipWriter};

    use super::super::IgnoreRules;
    use super::*;

    fn stored() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    }

    fn deflated() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
    }

    /// A zip of `(name, bytes)` entries, stored.
    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            w.start_file(*name, stored()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn open(bytes: Vec<u8>, options: &ArchiveOptions) -> Result<SafeArchive, ArchiveError> {
        SafeArchive::open(Cursor::new(bytes), options)
    }

    fn docs() -> ArchiveOptions {
        ArchiveOptions::documentation()
    }

    fn keys(a: &SafeArchive) -> Vec<&str> {
        a.entries().iter().map(|e| e.key.as_str()).collect()
    }

    /// Overwrite every occurrence of `from` with `to` (same length), in the
    /// local header and the central directory alike.
    fn patch(bytes: &mut [u8], from: &[u8], to: &[u8]) {
        assert_eq!(from.len(), to.len());
        let mut hits = 0;
        for i in 0..=bytes.len() - from.len() {
            if &bytes[i..i + from.len()] == from {
                bytes[i..i + from.len()].copy_from_slice(to);
                hits += 1;
            }
        }
        assert!(hits > 0, "nothing patched");
    }

    /// Rewrite the uncompressed size a one-entry zip declares, in its local
    /// header and its central directory record.
    fn declare_size(bytes: &mut [u8], size: u32) {
        let eocd = bytes.len() - 22;
        assert_eq!(bytes[eocd..eocd + 4], [0x50, 0x4b, 0x05, 0x06]);
        let cd = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(bytes[cd..cd + 4], [0x50, 0x4b, 0x01, 0x02]);
        assert_eq!(bytes[..4], [0x50, 0x4b, 0x03, 0x04]);
        bytes[22..26].copy_from_slice(&size.to_le_bytes());
        bytes[cd + 24..cd + 28].copy_from_slice(&size.to_le_bytes());
    }

    #[test]
    fn a_plain_archive_lists_keys_and_reads_them() {
        let bytes = zip_of(&[
            ("Handbook/index.md", b"# Handbook"),
            ("Handbook\\Setup\\vpn.md", b"# VPN"),
        ]);
        let mut a = open(bytes, &docs()).unwrap();
        assert_eq!(keys(&a), ["Handbook/index.md", "Handbook/Setup/vpn.md"]);
        assert_eq!(a.read("Handbook/Setup/vpn.md", 1024).unwrap(), b"# VPN");
        assert_eq!(a.bytes_read(), 5);
        assert_eq!(
            a.read("nope.md", 1024),
            Err(ArchiveError::NotFound {
                name: "nope.md".into()
            })
        );
    }

    #[test]
    fn opens_from_a_temp_file() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&zip_of(&[("a.md", b"hello")])).unwrap();
        let mut a = SafeArchive::open(file, &docs()).unwrap();
        assert_eq!(a.read_text("a.md", 100).unwrap().text, "hello");
    }

    #[test]
    fn something_that_isnt_a_zip_is_refused() {
        assert_eq!(
            open(b"not a zip at all".to_vec(), &docs()).unwrap_err(),
            ArchiveError::NotAZip { part: None }
        );
    }

    #[test]
    fn a_zip_bomb_is_refused_by_its_declared_ratio() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("bomb.md", deflated()).unwrap();
        w.write_all(&vec![0u8; 8 * 1024 * 1024]).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        assert!(bytes.len() < 100_000, "deflated zeros compress past 100:1");
        assert_eq!(
            open(bytes, &docs()).unwrap_err(),
            ArchiveError::Ratio {
                name: "bomb.md".into(),
                max: 100
            }
        );
    }

    #[test]
    fn a_bomb_that_understates_its_size_stops_at_the_declared_size() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("liar.md", deflated()).unwrap();
        w.write_all(&vec![0u8; 4 * 1024 * 1024]).unwrap();
        let mut bytes = w.finish().unwrap().into_inner();
        // Claim 1,000 bytes in both headers so the open-time check passes.
        declare_size(&mut bytes, 1000);
        let mut a = open(bytes, &docs()).unwrap();
        assert_eq!(a.entry("liar.md").unwrap().declared_size, 1000);
        // The zip crate stops at the declared size; the take bound
        // (`copy_limited`) is the second line.
        let err = a.read("liar.md", u64::MAX).unwrap_err();
        assert!(matches!(err, ArchiveError::Corrupt { .. }), "{err:?}");
        assert!(a.bytes_read() <= 1000, "inflated {}", a.bytes_read());
    }

    #[test]
    fn the_take_bound_stops_a_source_that_runs_past_its_allowance() {
        // An endless source, as a lying header with no crate check would be.
        let mut out = Vec::new();
        let (produced, outcome) = copy_limited(io::repeat(7), 100_000, &mut out);
        assert!(matches!(outcome, Err(Overrun::Over)));
        assert_eq!(produced, 100_001, "one byte past the allowance, no more");
        assert_eq!(out.len(), 65_536, "the chunk that went over isn't written");

        let mut out = Vec::new();
        let (produced, outcome) = copy_limited(&b"exactly"[..], 7, &mut out);
        assert!(outcome.is_ok());
        assert_eq!((produced, out.as_slice()), (7, &b"exactly"[..]));
    }

    #[test]
    fn a_traversal_entry_refuses_the_archive() {
        for name in ["../../etc/cron.d/x", "docs/../../x.md", "..\\x.md"] {
            let err = open(zip_of(&[("ok.md", b"x"), (name, b"x")]), &docs()).unwrap_err();
            assert!(
                matches!(
                    err,
                    ArchiveError::UnsafePath {
                        reason: "parent folder segment",
                        ..
                    }
                ),
                "{name}: {err:?}"
            );
        }
    }

    #[test]
    fn an_absolute_or_drive_path_refuses_the_archive() {
        for (name, reason) in [
            ("/etc/passwd", "absolute path"),
            ("C:/Windows/win.ini", "drive letter"),
        ] {
            let err = open(zip_of(&[(name, b"x")]), &docs()).unwrap_err();
            assert_eq!(
                err,
                ArchiveError::UnsafePath {
                    name: name.into(),
                    reason
                }
            );
        }
    }

    #[test]
    fn a_nul_in_a_name_refuses_the_archive() {
        let mut bytes = zip_of(&[("aXb.md", b"x")]);
        patch(&mut bytes, b"aXb.md", b"a\0b.md");
        assert!(matches!(
            open(bytes, &docs()).unwrap_err(),
            ArchiveError::UnsafePath {
                reason: "NUL in name",
                ..
            }
        ));
    }

    #[test]
    fn a_symlink_is_skipped_and_never_readable() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("a.md", stored()).unwrap();
        w.write_all(b"x").unwrap();
        w.add_symlink("passwd.md", "/etc/passwd", stored()).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        let mut a = open(bytes, &docs()).unwrap();
        assert_eq!(keys(&a), ["a.md"]);
        assert_eq!(
            a.skipped(),
            [Skipped {
                key: "passwd.md".into(),
                reason: SkipReason::Symlink
            }]
        );
        assert!(a.read("passwd.md", 1024).is_err());
    }

    #[test]
    fn an_encrypted_entry_refuses_the_archive() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("a.md", stored()).unwrap();
        w.write_all(b"x").unwrap();
        w.start_file(
            "secret.md",
            deflated().with_aes_encryption(AesMode::Aes256, "hunter2"),
        )
        .unwrap();
        w.write_all(b"secret").unwrap();
        let bytes = w.finish().unwrap().into_inner();
        assert_eq!(
            open(bytes, &docs()).unwrap_err(),
            ArchiveError::Encrypted {
                name: "secret.md".into()
            }
        );
    }

    #[test]
    fn too_many_entries_are_refused_before_listing() {
        let names: Vec<String> = (0..10_001).map(|i| format!("p/{i}.md")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
        let bytes = zip_of(&files);
        assert_eq!(
            open(bytes.clone(), &docs()).unwrap_err(),
            ArchiveError::TooManyEntries { max: 10_000 }
        );
        // The cap is the caller's.
        let mut roomy = docs();
        roomy.limits.max_entries = 20_000;
        assert_eq!(open(bytes, &roomy).unwrap().entries().len(), 10_001);
    }

    #[test]
    fn an_oversize_entry_fails_its_read_and_leaves_the_archive_open() {
        let big = vec![b'x'; 3000];
        let mut options = docs();
        options.limits.max_entry_bytes = 2000;
        let mut a = open(zip_of(&[("big.md", &big), ("small.md", b"ok")]), &options).unwrap();
        assert_eq!(
            a.read("big.md", u64::MAX),
            Err(ArchiveError::EntryTooLarge {
                name: "big.md".into(),
                max: 2000
            })
        );
        assert_eq!(a.bytes_read(), 0, "refused on its header before inflating");
        // A read may ask for less than the entry cap.
        assert_eq!(
            a.read("small.md", 1),
            Err(ArchiveError::EntryTooLarge {
                name: "small.md".into(),
                max: 1
            })
        );
        assert_eq!(a.read("small.md", 2).unwrap(), b"ok");
    }

    #[test]
    fn a_stored_entry_that_understates_its_size_is_not_read_past_it() {
        let real = vec![b'y'; 5000];
        let mut bytes = zip_of(&[("under.md", &real)]);
        declare_size(&mut bytes, 10);
        let mut a = open(bytes, &docs()).unwrap();
        let mut out = Vec::new();
        let err = a.copy_to("under.md", 100, &mut out).unwrap_err();
        assert!(matches!(err, ArchiveError::Corrupt { .. }), "{err:?}");
        assert!(out.len() <= 10);
    }

    #[test]
    fn the_total_budget_spans_every_read() {
        let mut options = docs();
        options.limits.max_total_bytes = 3000;
        let block = vec![b'z'; 2000];
        let mut a = open(zip_of(&[("a.md", &block), ("b.md", &block)]), &options).unwrap();
        assert_eq!(a.read("a.md", u64::MAX).unwrap().len(), 2000);
        assert_eq!(
            a.read("b.md", u64::MAX),
            Err(ArchiveError::TotalTooLarge { max: 3000 })
        );
    }

    #[test]
    fn names_that_collide_after_normalising_are_refused() {
        let err = open(zip_of(&[("a/b.md", b"1"), ("a\\b.md", b"2")]), &docs()).unwrap_err();
        assert_eq!(
            err,
            ArchiveError::Duplicate {
                name: "a/b.md".into()
            }
        );
    }

    #[test]
    fn a_raw_name_listed_twice_is_refused() {
        let mut bytes = zip_of(&[("a/b.md", b"1"), ("a/c.md", b"2")]);
        patch(&mut bytes, b"a/c.md", b"a/b.md");
        assert!(matches!(
            open(bytes, &docs()).unwrap_err(),
            ArchiveError::Duplicate { .. }
        ));
    }

    #[test]
    fn ignored_folders_are_skipped_but_reported() {
        let bytes = zip_of(&[
            ("vault/.obsidian/app.json", b"{}"),
            ("vault/node_modules/x/readme.md", b"x"),
            ("__MACOSX/vault/._a.md", b"x"),
            ("vault/site/index.html", b"x"),
            ("vault/notes/a.md", b"# A"),
        ]);
        let a = open(bytes.clone(), &docs()).unwrap();
        assert_eq!(keys(&a), ["vault/notes/a.md"]);
        assert_eq!(a.skipped().len(), 4);
        assert!(a.skipped().iter().all(|s| s.reason == SkipReason::Ignored));
        assert!(a
            .skipped()
            .iter()
            .any(|s| s.key == "vault/.obsidian/app.json"));

        // The workspace archive's rules keep everything but OS clutter.
        let single = ArchiveOptions::single(ArchiveLimits::default());
        assert_eq!(open(bytes.clone(), &single).unwrap().entries().len(), 4);
        let mut none = single;
        none.ignore = IgnoreRules::none();
        assert_eq!(open(bytes, &none).unwrap().entries().len(), 5);
    }

    #[test]
    fn folders_are_listed_apart_from_entries() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.add_directory("Empty/", stored()).unwrap();
        w.add_directory(".git/", stored()).unwrap();
        w.start_file("Full/a.md", stored()).unwrap();
        w.write_all(b"x").unwrap();
        let a = open(w.finish().unwrap().into_inner(), &docs()).unwrap();
        assert_eq!(a.folders(), ["Empty"]);
        assert_eq!(keys(&a), ["Full/a.md"]);
    }

    #[test]
    fn non_utf8_and_utf16_text_decode_with_flags() {
        let mut utf16 = vec![0xFF, 0xFE];
        for u in "# Réseau".encode_utf16() {
            utf16.extend(u.to_le_bytes());
        }
        let mut a = open(
            zip_of(&[("latin1.md", b"# R\xE9seau"), ("utf16.md", &utf16)]),
            &docs(),
        )
        .unwrap();
        let latin = a.read_text("latin1.md", 1024).unwrap();
        assert!(latin.lossy);
        assert_eq!(latin.text, "# R\u{FFFD}seau");
        let wide = a.read_text("utf16.md", 1024).unwrap();
        assert!(!wide.lossy);
        assert_eq!(wide.text, "# Réseau");
    }

    #[test]
    fn a_zip_of_zips_opens_as_one_split_export() {
        let part1 = zip_of(&[("Export/A.md", b"# A"), ("Export/img/a.png", b"png")]);
        let part2 = zip_of(&[("Export/B.md", b"# B")]);
        let outer = zip_of(&[
            ("Export-Part-1.zip", &part1),
            ("Export-Part-2.ZIP", &part2),
            // macOS adds a resource fork per file; it's ignored before the
            // "nothing but zips" check.
            ("__MACOSX/._Export-Part-1.zip", b"fork"),
        ]);
        let mut a = open(outer, &docs()).unwrap();
        assert_eq!(keys(&a), ["Export/A.md", "Export/img/a.png", "Export/B.md"]);
        assert_eq!(a.split_parts(), ["Export-Part-1.zip", "Export-Part-2.ZIP"]);
        assert_eq!(
            a.entry("Export/B.md").unwrap().part.as_deref(),
            Some("Export-Part-2.ZIP")
        );
        assert_eq!(a.read("Export/B.md", 100).unwrap(), b"# B");
        assert!(
            a.bytes_read() > part1.len() as u64,
            "parts count to the total"
        );
    }

    #[test]
    fn nested_zips_beside_other_files_stay_ordinary_entries() {
        let inner = zip_of(&[("x.md", b"x")]);
        let outer = zip_of(&[("attachments.zip", &inner), ("readme.md", b"# Hi")]);
        let a = open(outer, &docs()).unwrap();
        assert_eq!(keys(&a), ["attachments.zip", "readme.md"]);
        assert!(a.split_parts().is_empty());
    }

    #[test]
    fn only_one_level_of_nesting_is_opened() {
        let deepest = zip_of(&[("deep.md", b"x")]);
        let part = zip_of(&[("inner.zip", &deepest)]);
        let outer = zip_of(&[("part.zip", &part)]);
        let a = open(outer, &docs()).unwrap();
        assert_eq!(keys(&a), ["inner.zip"]);
    }

    #[test]
    fn a_caller_that_never_nests_keeps_the_parts_as_entries() {
        let part = zip_of(&[("a.md", b"x")]);
        let outer = zip_of(&[("part.zip", &part)]);
        let a = open(outer, &ArchiveOptions::single(ArchiveLimits::default())).unwrap();
        assert_eq!(keys(&a), ["part.zip"]);
    }

    #[test]
    fn a_split_export_is_refused_for_what_any_part_holds() {
        let ok = zip_of(&[("a.md", b"x")]);
        let not_zip = b"PK? no".to_vec();
        assert_eq!(
            open(zip_of(&[("1.zip", &ok), ("2.zip", &not_zip)]), &docs()).unwrap_err(),
            ArchiveError::NotAZip {
                part: Some("2.zip".into())
            }
        );

        let same = zip_of(&[("a.md", b"y")]);
        assert_eq!(
            open(zip_of(&[("1.zip", &ok), ("2.zip", &same)]), &docs()).unwrap_err(),
            ArchiveError::Duplicate {
                name: "a.md".into()
            }
        );

        let evil = zip_of(&[("../a.md", b"x")]);
        assert!(matches!(
            open(zip_of(&[("1.zip", &evil)]), &docs()).unwrap_err(),
            ArchiveError::UnsafePath { .. }
        ));

        // The entry cap counts every part's entries.
        let mut tight = docs();
        tight.limits.max_entries = 3;
        let two = zip_of(&[("b.md", b"x"), ("c.md", b"x")]);
        assert_eq!(
            open(zip_of(&[("1.zip", &ok), ("2.zip", &two)]), &tight).unwrap_err(),
            ArchiveError::TooManyEntries { max: 3 }
        );
    }

    #[test]
    fn a_bomb_inside_a_part_is_refused() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("bomb.md", deflated()).unwrap();
        w.write_all(&vec![0u8; 8 * 1024 * 1024]).unwrap();
        let part = w.finish().unwrap().into_inner();
        assert!(matches!(
            open(zip_of(&[("1.zip", &part)]), &docs()).unwrap_err(),
            ArchiveError::Ratio { .. }
        ));
    }

    #[test]
    fn a_non_utf8_name_is_read_and_flagged() {
        let mut bytes = zip_of(&[("caf_.md", b"x")]);
        patch(&mut bytes, b"caf_.md", b"caf\x82.md");
        let a = open(bytes, &docs()).unwrap();
        let entry = &a.entries()[0];
        assert!(entry.name_guessed);
        assert_eq!(entry.key, "café.md", "CP437 0x82 is é");
    }
}
