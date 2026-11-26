//! The bundle: a source-neutral copy of another helpdesk's data, the contract
//! between a connector (which reads Zendesk, Freshdesk, ...) and the loader
//! (which writes Nosdesk). Connectors never write to Nosdesk; the loader never
//! talks to a source. A new source is a connector that fills a bundle.
//!
//! On disk a bundle is a directory:
//!
//! ```text
//! manifest.json   Manifest: source, scope, counts, notes
//! state.json      FetchState: how far a connector got, so a fetch resumes
//! people.jsonl    one Person per line
//! tickets.jsonl   one Ticket per line, its conversation embedded
//! files/<id>      attachment bytes, named by the attachment's source id
//! ```
//!
//! A ticket carries its comments, and a comment its attachments, so the loader
//! takes tickets in batches without joining files, and a batch is
//! self-contained. (The 1.2 design kept comments in a file of their own; one
//! line per ticket is simpler to batch and resume, and the format is ours.)

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Bumped when a field changes meaning. The loader refuses a bundle it
/// doesn't know rather than misreading it.
pub const FORMAT_VERSION: u32 = 1;

pub const MANIFEST: &str = "manifest.json";
pub const STATE: &str = "state.json";
pub const PEOPLE: &str = "people.jsonl";
pub const TICKETS: &str = "tickets.jsonl";
pub const FILES: &str = "files";
pub const LOCK: &str = ".lock";

/// The bundle's lock, held until dropped (closing the file releases it).
pub struct BundleLock {
    _file: File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    /// `zendesk`, `freshdesk`, ... The id map's `source`.
    pub source: String,
    /// The source instance, e.g. the Zendesk subdomain. The id map's
    /// `source_scope`, so two accounts imported into one workspace never
    /// share ids.
    pub source_scope: String,
    pub extracted_at: DateTime<Utc>,
    /// Records written, by kind: `people`, `tickets`, `comments`,
    /// `attachments`, `attachments_missing`.
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
    /// What the connector couldn't carry, in words, for the report
    /// ("custom field 'Region' kept as a note", "3 attachments off-host").
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PersonRole {
    /// Raises tickets: a customer, an employee asking IT.
    Requester,
    /// Works tickets.
    Agent,
    /// Works tickets and administers the source.
    Admin,
}

impl PersonRole {
    pub fn is_staff(self) -> bool {
        matches!(self, PersonRole::Agent | PersonRole::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Person {
    pub source_id: String,
    /// Lower-cased. None for sources that allow people without one (a
    /// Zendesk end-user known only by phone); the loader gives those a
    /// placeholder author.
    pub email: Option<String>,
    pub name: String,
    pub role: PersonRole,
    /// Suspended or deleted at the source. Still imported (their history is
    /// real), never given access.
    #[serde(default = "yes")]
    pub active: bool,
    /// The person's organisation or company at the source, by name.
    #[serde(default)]
    pub organization: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn yes() -> bool {
    true
}

/// Where a ticket stands, in words every helpdesk has. The loader maps each
/// to a Nosdesk workflow state by category; the source's own status name is
/// kept on the ticket record for the report.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// Not yet looked at.
    New,
    /// Being worked.
    Open,
    /// Waiting on the requester.
    Pending,
    /// Waiting on someone else (a vendor, another team).
    OnHold,
    /// Answered; may still be reopened.
    Solved,
    /// Finished for good.
    Closed,
}

impl StatusKind {
    pub fn is_finished(self) -> bool {
        matches!(self, StatusKind::Solved | StatusKind::Closed)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PriorityLevel {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    Plain,
    /// HTML as the source rendered it. The loader sanitises it.
    Html,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Body {
    pub format: BodyFormat,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub source_id: String,
    pub file_name: String,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    /// True when the bytes are at `files/<source_id>`. False when the
    /// connector couldn't fetch them; `missing_reason` says why, and the
    /// loader reports it instead of failing the ticket.
    pub stored: bool,
    #[serde(default)]
    pub missing_reason: Option<String>,
    /// Where the connector fetched it from, so a resumed fetch can try a
    /// missing file again. It can carry a token: it stays in the bundle and
    /// never reaches a report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// The file is missing for a reason that may pass (a dropped
    /// connection, a short read), so a resumed fetch tries it again.
    #[serde(default)]
    pub retry: bool,
    /// Later runs that tried it again and failed. After
    /// [`MAX_FILE_RETRIES`] it is left missing.
    #[serde(default)]
    pub retries: u32,
}

/// Later runs that try a missing file again before it is left missing.
pub const MAX_FILE_RETRIES: u32 = 3;

impl Attachment {
    /// Missing, with a link, for a reason that may pass, and not given up on.
    pub fn retryable(&self) -> bool {
        !self.stored && self.retry && self.source_url.is_some() && self.retries < MAX_FILE_RETRIES
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Comment {
    pub source_id: String,
    /// The author's `Person::source_id`. None when the source doesn't say.
    pub author: Option<String>,
    /// False for an internal note.
    pub public: bool,
    pub body: Body,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ticket {
    pub source_id: String,
    /// The number people quote ("#1234"). Kept as the Nosdesk number when
    /// the workspace has room for it.
    pub number: Option<i64>,
    pub subject: String,
    pub status: StatusKind,
    /// The source's own status name, for the report ("hold", "Awaiting
    /// vendor").
    pub status_label: String,
    pub priority: Option<PriorityLevel>,
    /// The source's type or category ("incident", "question").
    #[serde(default)]
    pub ticket_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub requester: Option<String>,
    pub assignee: Option<String>,
    /// People copied in or following.
    #[serde(default)]
    pub watchers: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When it was solved or closed, if the source records it.
    #[serde(default)]
    pub solved_at: Option<DateTime<Utc>>,
    /// Source fields Nosdesk has no column for, as label → value. Written
    /// to one internal note on the ticket.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// The conversation, oldest first. The first comment is the request.
    pub comments: Vec<Comment>,
}

/// How far a connector got, so an interrupted fetch picks up where it
/// stopped instead of starting over. Each source keeps its own cursor shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FetchState {
    pub people_done: bool,
    /// The source's cursor for people changed since the people were fetched,
    /// so the people the tickets fetched later name can be fetched too.
    #[serde(default)]
    pub people_cursor: Option<String>,
    /// The source's cursor for the next page of tickets.
    pub ticket_cursor: Option<String>,
    /// Byte length of tickets.jsonl when the cursor was saved. A resumed
    /// fetch truncates back to it, dropping a half-written page.
    pub tickets_len: u64,
    pub tickets_done: bool,
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// A bundle directory.
#[derive(Debug, Clone)]
pub struct Bundle {
    root: PathBuf,
}

impl Bundle {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Bundle { root: root.into() }
    }

    /// Creates the bundle directory, or opens an existing one. A bundle is a
    /// copy of a customer's helpdesk, so its directories are created
    /// readable by their owner only, from the start.
    pub fn create(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        private_dir(&root)?;
        private_dir(&root.join(FILES))?;
        Ok(Bundle { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn file_path(&self, attachment_source_id: &str) -> PathBuf {
        self.root
            .join(FILES)
            .join(safe_file_name(attachment_source_id))
    }

    /// [`Self::file_path`] for writing: refused unless `files/` is a real
    /// directory, not a link to somewhere else.
    pub fn file_path_to_write(&self, attachment_source_id: &str) -> io::Result<PathBuf> {
        self.files_dir()?;
        Ok(self.file_path(attachment_source_id))
    }

    fn files_dir(&self) -> io::Result<PathBuf> {
        let dir = self.root.join(FILES);
        if !fs::symlink_metadata(&dir)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the bundle's files/ is not a directory",
            ));
        }
        Ok(dir)
    }

    /// Opens an attachment's bytes for reading, with their length. Refuses
    /// anything but a regular file inside the bundle: a link (`files/123 ->
    /// /etc/passwd` in a bundle from elsewhere) is not followed.
    pub fn open_file(&self, attachment_source_id: &str) -> io::Result<(File, u64)> {
        let path = self.files_dir()?.join(safe_file_name(attachment_source_id));
        // Checked before opening: opening a FIFO would wait for a writer.
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a regular file in the bundle",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Not followed if it became a link since, and never waiting if
            // it became a FIFO: the check after opening refuses either.
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&path)?;
        let meta = file.metadata()?;
        let inside = fs::canonicalize(&path)?.starts_with(fs::canonicalize(&self.root)?);
        if !meta.is_file() || !inside {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a regular file in the bundle",
            ));
        }
        Ok((file, meta.len()))
    }

    pub fn read_manifest(&self) -> io::Result<Manifest> {
        let m: Manifest = read_json(&self.root.join(MANIFEST))?;
        if m.format_version != FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "bundle format {} is not the {} this server reads",
                    m.format_version, FORMAT_VERSION
                ),
            ));
        }
        Ok(m)
    }

    pub fn write_manifest(&self, manifest: &Manifest) -> io::Result<()> {
        write_json_atomic(&self.root.join(MANIFEST), manifest)
    }

    pub fn read_state(&self) -> io::Result<FetchState> {
        let path = self.root.join(STATE);
        if !path.exists() {
            return Ok(FetchState::default());
        }
        read_json(&path)
    }

    pub fn write_state(&self, state: &FetchState) -> io::Result<()> {
        write_json_atomic(&self.root.join(STATE), state)
    }

    /// Appends to people.jsonl. `fresh` starts the file over.
    pub fn people_writer(&self, fresh: bool) -> io::Result<JsonlWriter> {
        JsonlWriter::open(&self.root.join(PEOPLE), fresh, u64::MAX)
    }

    /// Appends to tickets.jsonl from `keep_len` bytes, dropping anything a
    /// previous, interrupted fetch wrote after its last checkpoint.
    pub fn tickets_writer(&self, keep_len: u64) -> io::Result<JsonlWriter> {
        JsonlWriter::open(&self.root.join(TICKETS), keep_len == 0, keep_len)
    }

    pub fn people(&self) -> io::Result<JsonlReader<Person>> {
        JsonlReader::open(&self.root.join(PEOPLE))
    }

    pub fn tickets(&self) -> io::Result<JsonlReader<Ticket>> {
        JsonlReader::open(&self.root.join(TICKETS))
    }

    /// tickets.jsonl as raw lines, for a rewrite that passes through what it
    /// doesn't change (or can't read) untouched.
    pub fn ticket_lines(&self) -> io::Result<io::Lines<BufReader<File>>> {
        Ok(BufReader::new(File::open(self.root.join(TICKETS))?).lines())
    }

    /// Whether any attachment is missing for a reason that may have passed,
    /// with a link to try again. Reads one ticket at a time.
    pub fn has_retryable_files(&self) -> io::Result<bool> {
        for t in self.tickets()?.filter_map(Result::ok) {
            let retryable = t
                .comments
                .iter()
                .any(|c| c.attachments.iter().any(Attachment::retryable));
            if retryable {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The bundle's lock, without waiting: None while another run holds it.
    /// Held for as long as the returned guard lives; a run that rewrites
    /// the bundle's files takes it first, so two runs never interleave.
    pub fn try_lock(&self) -> io::Result<Option<BundleLock>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(LOCK))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
        Ok(Some(BundleLock { _file: file }))
    }

    /// A replacement for tickets.jsonl, written beside it and swapped in
    /// whole by [`TicketsRewrite::finish`].
    pub fn tickets_rewrite(&self) -> io::Result<TicketsRewrite> {
        let path = self.root.join(TICKETS);
        // A name of its own, so nothing else writing beside it shares the
        // file.
        let tmp = path.with_extension(format!(
            "jsonl.{}-{}.tmp",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        ));
        Ok(TicketsRewrite {
            out: JsonlWriter::open(&tmp, true, 0)?,
            tmp,
            path,
        })
    }
}

pub struct TicketsRewrite {
    out: JsonlWriter,
    tmp: PathBuf,
    path: PathBuf,
}

impl TicketsRewrite {
    pub fn write(&mut self, ticket: &Ticket) -> io::Result<()> {
        self.out.write(ticket)
    }

    /// A line kept as it was.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.out.out.write_all(line.as_bytes())?;
        self.out.out.write_all(b"\n")?;
        self.out.len += line.len() as u64 + 1;
        Ok(())
    }

    /// Swaps the new file in and returns its length: the checkpoint a
    /// resumed fetch truncates back to.
    pub fn finish(mut self) -> io::Result<u64> {
        let len = self.out.checkpoint()?;
        drop(self.out);
        fs::rename(&self.tmp, &self.path)?;
        Ok(len)
    }
}

/// Creates `path` (and any missing parents) as owner-only directories, or
/// makes an existing one owner-only. Refuses a link.
fn private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a directory", path.display()),
            ));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

/// Attachment ids come from the source; keep them to a safe file name.
fn safe_file_name(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(128)
        .collect();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    let f = File::open(path)?;
    serde_json::from_reader(BufReader::new(f))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = BufWriter::new(File::create(&tmp)?);
        serde_json::to_writer_pretty(&mut f, value).map_err(io::Error::other)?;
        f.flush()?;
        f.get_ref().sync_all()?;
    }
    fs::rename(tmp, path)
}

pub struct JsonlWriter {
    out: BufWriter<File>,
    len: u64,
}

impl JsonlWriter {
    fn open(path: &Path, fresh: bool, keep_len: u64) -> io::Result<Self> {
        let mut f = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(fresh)
            .open(path)?;
        let len = if fresh {
            0
        } else {
            keep_len.min(f.metadata()?.len())
        };
        f.set_len(len)?;
        f.seek(SeekFrom::Start(len))?;
        Ok(JsonlWriter {
            out: BufWriter::new(f),
            len,
        })
    }

    pub fn write<T: Serialize>(&mut self, record: &T) -> io::Result<()> {
        let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
        line.push(b'\n');
        self.out.write_all(&line)?;
        self.len += line.len() as u64;
        Ok(())
    }

    /// Flushes to disk and returns the file's length: the checkpoint a
    /// resumed fetch truncates back to.
    pub fn checkpoint(&mut self) -> io::Result<u64> {
        self.out.flush()?;
        self.out.get_ref().sync_data()?;
        Ok(self.len)
    }
}

/// Reads a JSONL file one record at a time, so a bundle of any size streams.
pub struct JsonlReader<T> {
    lines: io::Lines<BufReader<File>>,
    line_no: usize,
    _t: std::marker::PhantomData<T>,
}

impl<T> JsonlReader<T> {
    fn open(path: &Path) -> io::Result<Self> {
        Ok(JsonlReader {
            lines: BufReader::new(File::open(path)?).lines(),
            line_no: 0,
            _t: std::marker::PhantomData,
        })
    }
}

impl<T: for<'de> Deserialize<'de>> Iterator for JsonlReader<T> {
    type Item = io::Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.lines.next()? {
                Ok(l) => l,
                Err(e) => return Some(Err(e)),
            };
            self.line_no += 1;
            if line.trim().is_empty() {
                continue;
            }
            return Some(serde_json::from_str(&line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("line {}: {e}", self.line_no),
                )
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(id: &str) -> Ticket {
        Ticket {
            source_id: id.into(),
            number: Some(id.parse().unwrap_or(1)),
            subject: format!("Ticket {id}"),
            status: StatusKind::Open,
            status_label: "open".into(),
            priority: Some(PriorityLevel::Normal),
            ticket_type: None,
            tags: vec!["vpn".into()],
            requester: Some("u1".into()),
            assignee: None,
            watchers: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            solved_at: None,
            fields: BTreeMap::new(),
            comments: vec![Comment {
                source_id: format!("c{id}"),
                author: Some("u1".into()),
                public: true,
                body: Body {
                    format: BodyFormat::Html,
                    value: "<p>Hi</p>".into(),
                },
                created_at: Utc::now(),
                attachments: vec![],
            }],
        }
    }

    #[test]
    fn tickets_round_trip_and_resume_drops_a_half_written_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let b = Bundle::create(dir.path()).expect("create");

        let mut w = b.tickets_writer(0).expect("writer");
        w.write(&ticket("1")).expect("write");
        let checkpoint = w.checkpoint().expect("checkpoint");
        w.write(&ticket("2")).expect("write");
        w.checkpoint().expect("checkpoint");
        drop(w);

        // A resumed fetch from the first checkpoint loses ticket 2 and
        // writes it again.
        let mut w = b.tickets_writer(checkpoint).expect("writer");
        w.write(&ticket("3")).expect("write");
        w.checkpoint().expect("checkpoint");
        drop(w);

        let ids: Vec<String> = b
            .tickets()
            .expect("reader")
            .map(|t| t.expect("ticket").source_id)
            .collect();
        assert_eq!(ids, vec!["1", "3"]);
    }

    #[test]
    fn manifest_of_another_format_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let b = Bundle::create(dir.path()).expect("create");
        let mut m = Manifest {
            format_version: FORMAT_VERSION,
            source: "zendesk".into(),
            source_scope: "acme".into(),
            extracted_at: Utc::now(),
            counts: BTreeMap::new(),
            notes: vec![],
        };
        b.write_manifest(&m).expect("write");
        assert_eq!(b.read_manifest().expect("read"), m);
        m.format_version = FORMAT_VERSION + 1;
        b.write_manifest(&m).expect("write");
        assert!(b.read_manifest().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_file_is_not_read_and_the_bundle_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let b = Bundle::create(dir.path().join("b")).expect("create");
        let mode = fs::metadata(b.root()).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let secret = dir.path().join("secret");
        fs::write(&secret, b"operator's environment").expect("secret");
        std::os::unix::fs::symlink(&secret, b.file_path("123")).expect("link");
        assert!(b.open_file("123").is_err(), "a link is not followed");
        // A FIFO is refused at once, not waited on.
        let fifo = b.file_path("126");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(made.success());
        assert!(b.open_file("126").is_err());
        fs::write(b.file_path("124"), b"bytes").expect("file");
        assert_eq!(b.open_file("124").expect("file").1, 5);
        // A files/ that is itself a link is refused too.
        let other = tempfile::tempdir().expect("other");
        fs::write(other.path().join("125"), b"elsewhere").expect("file");
        fs::remove_dir_all(b.root().join(FILES)).expect("rm");
        std::os::unix::fs::symlink(other.path(), b.root().join(FILES)).expect("link dir");
        assert!(b.open_file("125").is_err());
        assert!(b.file_path_to_write("125").is_err());
    }

    #[test]
    fn attachment_ids_become_safe_file_names() {
        let b = Bundle::open("/tmp/x");
        assert!(b
            .file_path("../../etc/passwd")
            .ends_with("______etc_passwd"));
        assert!(b.file_path("12345").ends_with("12345"));
    }
}
