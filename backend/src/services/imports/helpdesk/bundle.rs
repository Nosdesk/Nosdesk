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

    pub fn create(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join(FILES))?;
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
        JsonlWriter::open(&self.root.join(PEOPLE), fresh, 0)
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

    #[test]
    fn attachment_ids_become_safe_file_names() {
        let b = Bundle::open("/tmp/x");
        assert!(b
            .file_path("../../etc/passwd")
            .ends_with("______etc_passwd"));
        assert!(b.file_path("12345").ends_with("12345"));
    }
}
