//! Reading a Zendesk full JSON export (Admin Center → Account → Tools →
//! Reports → Export, "JSON") instead of the API: the way to bring an
//! account across after Zendesk turns API tokens off, or when the customer
//! would rather hand over a file than a credential.
//!
//! UNVERIFIED FORMAT. Zendesk documents only this much
//! (support.zendesk.com/hc/en-us/articles/4408886165402, checked
//! 2026-11-22): the download is a zip of NDJSON files named
//! `export_YYYY_MM_DD_<id>_<n>`, one object per line, a ticket written as
//! `{"ticket": {...}}`; tickets over 1 MB arrive without their comments;
//! record order is not guaranteed. It does not say which file holds what,
//! nor the fields inside. So this reader is deliberately tolerant:
//!
//! - every entry in the zip is read, whatever its name;
//! - each line may be `{"ticket": {...}}`, `{"user": {...}}`,
//!   `{"organization": {...}}`, or a bare record, which is recognised by
//!   its shape (or, failing that, by its file name);
//! - an entry that isn't NDJSON but one JSON document (an array, or
//!   `{"tickets": [...]}`) is read whole, up to a size limit, past which the
//!   read fails rather than guess; when the whole entry doesn't parse
//!   either, it is read line by line after all, so one bad first line costs
//!   only that line;
//! - a byte order mark is skipped, and a record of no shape the reader
//!   knows is counted and reported, and a zip with records but no ticket
//!   the reader recognises is refused;
//! - record fields are assumed to match the API's (tickets carrying a
//!   `comments` array shaped like the comments endpoint's), which is how
//!   Zendesk's legacy JSON export looked. A ticket without comments gets
//!   its `description` as the first comment, and a note says so.
//!
//! Attachments are downloaded from their `content_url` under the same rules
//! as the API path. No credential is needed: with a source from
//! [`ZendeskSource::for_export`] downloads go unsigned, and one that needs
//! sign-in is listed as missing with that reason. The read starts the bundle over: an interrupted read
//! is simply run again (attachments already on disk are kept).

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{api, map, Run, ZendeskSource};
use crate::services::imports::helpdesk::bundle::{
    Body, BodyFormat, Bundle, Comment, FetchState, Manifest, PeopleSource,
};
use crate::services::imports::helpdesk::source::{FetchProgress, SourceError};

/// How much the reader takes at once.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// A line longer than this is not a record we can carry; it is skipped
    /// and counted. Zendesk keeps ticket records under about 1 MB.
    line: u64,
    /// The most read of an entry that is one JSON document rather than
    /// NDJSON.
    document: u64,
}

const LIMITS: Limits = Limits {
    line: 64 * 1024 * 1024,
    document: 256 * 1024 * 1024,
};
/// Tickets written between checkpoints of tickets.jsonl.
const CHECKPOINT_EVERY: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ticket,
    User,
    Organization,
}

/// Reads the export at `zip_path` into `bundle`, starting it over, and
/// writes the manifest. `source` supplies the scope (the subdomain) and the
/// client for attachment downloads; its credential is used, as on the API
/// path, only for Zendesk's own hosts, and also to look up custom field
/// titles, which the export doesn't carry (without them a field is
/// labelled by id).
pub async fn read_export(
    source: &ZendeskSource,
    zip_path: &Path,
    bundle: &Bundle,
    progress: &(dyn Fn(&FetchProgress) + Send + Sync),
    cancel: CancellationToken,
) -> Result<Manifest, SourceError> {
    // Starting over: a manifest from an earlier read would make a half
    // rewritten bundle look complete.
    match std::fs::remove_file(
        bundle
            .root()
            .join(crate::services::imports::helpdesk::bundle::MANIFEST),
    ) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut state = FetchState::default();
    bundle.write_state(&state)?;
    let mut run = Run::new(&state, progress);
    let mut notes = vec!["Read from a Zendesk JSON export, not the API".to_string()];

    // Pass one: organisations and people, which fit in memory.
    let path = zip_path.to_path_buf();
    let (orgs, users, people_tally) = tokio::task::spawn_blocking(move || read_people(&path))
        .await
        .map_err(|e| SourceError::Protocol(format!("export reader stopped: {e}")))??;
    let orgs = map::organizations(orgs);
    let mut writer = bundle.people_writer(true)?;
    let mut seen = HashSet::new();
    let mut people = 0u64;
    for u in &users {
        if seen.insert(u.id) {
            writer.write(&map::person(u, &orgs))?;
            people += 1;
        }
    }
    writer.checkpoint()?;
    drop(users);
    run.counts.insert("people".into(), people);
    run.progress.people = people;
    run.report(source);
    state.people_done = true;

    let fields = if !source.http.has_credential() {
        notes.push("Custom field titles need API access; without a Zendesk credential fields are labelled by id".into());
        map::FieldCatalog::default()
    } else {
        match source
            .list_all(
                "/api/v2/ticket_fields.json",
                |p: api::TicketFieldsPage| (p.ticket_fields, p.meta),
                &cancel,
            )
            .await
        {
            Ok(f) => map::FieldCatalog::new(f),
            Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
            Err(e) => {
                notes.push(format!(
                    "Custom field titles unavailable ({e}); fields are labelled by id"
                ));
                map::FieldCatalog::default()
            }
        }
    };

    // Pass two: tickets, streamed from a reader thread so an export of any
    // size never sits in memory.
    let (tx, mut rx) = mpsc::channel::<api::Ticket>(64);
    let path = zip_path.to_path_buf();
    let reader = tokio::task::spawn_blocking(move || read_tickets(&path, &tx));

    let mut writer = bundle.tickets_writer(0)?;
    let mut seen = HashSet::new();
    let mut since_checkpoint = 0u64;
    loop {
        let t = tokio::select! {
            t = rx.recv() => t,
            _ = cancel.cancelled() => return Err(SourceError::Cancelled),
        };
        let Some(t) = t else { break };
        if map::is_deleted(&t) {
            *run.counts
                .entry("deleted_tickets_skipped".into())
                .or_default() += 1;
            continue;
        }
        if !seen.insert(t.id) {
            *run.counts
                .entry("duplicate_tickets_skipped".into())
                .or_default() += 1;
            continue;
        }
        let Some(created_at) = map::ticket_created_at(&t) else {
            *run.counts
                .entry("tickets_without_dates".into())
                .or_default() += 1;
            continue;
        };
        let raw = match &t.comments {
            Some(c) if !c.is_empty() => c.clone(),
            _ => {
                *run.counts
                    .entry("tickets_without_comments".into())
                    .or_default() += 1;
                Vec::new()
            }
        };
        let (mut ticket, mut stats) = source
            .assemble(bundle, &t, created_at, None, &fields, &raw, &cancel)
            .await?;
        if ticket.comments.is_empty() {
            if let Some(c) = description_comment(&t, created_at) {
                ticket.comments.push(c);
                stats.comments += 1;
            }
        }
        writer.write(&ticket)?;
        stats.tickets = 1;
        run.add(&stats);
        since_checkpoint += 1;
        if since_checkpoint >= CHECKPOINT_EVERY {
            since_checkpoint = 0;
            writer.checkpoint()?;
            run.report(source);
        }
    }
    let ticket_tally = reader
        .await
        .map_err(|e| SourceError::Protocol(format!("export reader stopped: {e}")))??;
    state.tickets_len = writer.checkpoint()?;
    state.tickets_done = true;
    run.report(source);

    let tickets = run.counts.get("tickets").copied().unwrap_or(0);
    if tickets == 0 && (people_tally.records > 0 || ticket_tally.records > 0) {
        return Err(SourceError::Protocol(format!(
            "the export holds {} records but no ticket this reader recognises; \
             it may not be a Zendesk JSON export",
            people_tally.records.max(ticket_tally.records)
        )));
    }
    let unreadable = people_tally.unreadable + ticket_tally.unreadable;
    if unreadable > 0 {
        notes.push(format!(
            "{unreadable} lines of the export could not be read and were skipped"
        ));
    }
    let unrecognised = people_tally.unrecognised.max(ticket_tally.unrecognised);
    if unrecognised > 0 {
        notes.push(format!(
            "{unrecognised} records of the export were of no kind this reader knows and were skipped"
        ));
    }
    state.counts = run.counts.clone();
    state.notes = notes;
    bundle.write_state(&state)?;
    // People come from the zip alone, credential or not: Zendesk was never
    // asked for someone the zip lacks.
    let manifest = run.manifest("zendesk", &source.scope, &state.notes, PeopleSource::Export);
    bundle.write_manifest(&manifest)?;
    Ok(manifest)
}

/// The ticket's description as its opening comment, for a ticket the
/// export sent without its conversation.
fn description_comment(
    t: &api::Ticket,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Option<Comment> {
    let text = t.description.as_ref().filter(|d| !d.trim().is_empty())?;
    Some(Comment {
        source_id: format!("{}-description", t.id),
        author: t.requester_id.or(t.submitter_id).map(|id| id.to_string()),
        public: true,
        body: Body {
            format: BodyFormat::Plain,
            value: map::text(text),
        },
        created_at,
        attachments: vec![],
    })
}

fn zip_error(e: zip::result::ZipError) -> SourceError {
    SourceError::Protocol(format!("export zip: {e}"))
}

/// What a pass over the zip met besides the records it handed on.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    /// Non-empty lines (or whole documents) seen.
    records: u64,
    /// Lines that couldn't be read: not JSON, too long, the wrong shape.
    unreadable: u64,
    /// JSON records of no kind the reader knows.
    unrecognised: u64,
    /// Records of a kind the reader knows, wanted or not.
    recognised: u64,
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Visits every record of every entry in the zip. `want` says which kinds
/// the caller needs; the rest are passed over without being decoded.
fn each_record(
    path: &Path,
    want: &[Kind],
    limits: Limits,
    mut visit: impl FnMut(Kind, Value) -> bool,
) -> Result<Tally, SourceError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    let mut tally = Tally::default();
    for i in 0..archive.len() {
        let (is_dir, name, hint) = {
            let entry = archive.by_index(i).map_err(zip_error)?;
            let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
            (entry.is_dir(), name.clone(), kind_from_name(&name))
        };
        if is_dir {
            continue;
        }
        // A line that parses as JSON isn't a record: a pretty-printed
        // document read line by line yields scalars (`2`, `"vip"`). Only a
        // record of a known kind counts as something read.
        let readable = |t: &Tally| t.recognised;
        let at_start = readable(&tally);
        let mut overlong = false;
        let mut reader = BufReader::new(archive.by_index(i).map_err(zip_error)?);
        match read_lines(
            &mut reader,
            true,
            want,
            hint,
            limits,
            &mut tally,
            &mut overlong,
            &mut visit,
        )? {
            Lines::Stopped => return Ok(tally),
            Lines::Done => {
                // An entry whose first record is too long to read, and holds
                // nothing else, is a document too big to take, not noise.
                if overlong && readable(&tally) == at_start {
                    return Err(too_big(&name, limits.line));
                }
            }
            Lines::NotNdjson(first) => {
                // Perhaps one JSON document over many lines: read it whole,
                // up to the limit.
                let mut doc = first;
                doc.push(b'\n');
                let read = reader
                    .by_ref()
                    .take(limits.document + 1)
                    .read_to_end(&mut doc)?;
                let oversized = read as u64 > limits.document;
                if !oversized {
                    if let Ok(v) = serde_json::from_slice::<Value>(&doc) {
                        tally.records += 1;
                        if !hand_on(v, hint, want, &mut tally, &mut visit) {
                            return Ok(tally);
                        }
                        continue;
                    }
                }
                // Not one document either: line by line after all, the bad
                // first line counted as such.
                drop(reader);
                let mut lines = BufReader::new(archive.by_index(i).map_err(zip_error)?);
                if let Lines::Stopped = read_lines(
                    &mut lines,
                    false,
                    want,
                    hint,
                    limits,
                    &mut tally,
                    &mut overlong,
                    &mut visit,
                )? {
                    return Ok(tally);
                }
                // Nothing in it read line by line either: one document over
                // the limit, which the reader refuses rather than skip.
                if oversized && readable(&tally) == at_start {
                    return Err(too_big(&name, limits.document));
                }
            }
        }
    }
    Ok(tally)
}

fn too_big(entry: &str, limit: u64) -> SourceError {
    SourceError::Protocol(format!(
        "export entry {entry} holds a record over {} MB, more than this reader takes",
        limit.div_ceil(1024 * 1024)
    ))
}

/// How a line-by-line read of an entry ended.
enum Lines {
    Done,
    /// The visitor asked to stop.
    Stopped,
    /// The first line isn't JSON on its own: these are its bytes, and the
    /// reader is just past them.
    NotNdjson(Vec<u8>),
}

/// Reads `reader` one line at a time. With `first_may_be_document`, a first
/// line that doesn't parse ends the read as [`Lines::NotNdjson`]; otherwise
/// it counts as unreadable like any other.
#[allow(clippy::too_many_arguments)]
fn read_lines(
    reader: &mut impl BufRead,
    first_may_be_document: bool,
    want: &[Kind],
    hint: Option<Kind>,
    limits: Limits,
    tally: &mut Tally,
    overlong_first: &mut bool,
    visit: &mut impl FnMut(Kind, Value) -> bool,
) -> Result<Lines, SourceError> {
    let mut first = true;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let Some(complete) = read_line_capped(reader, &mut buf, limits.line)? else {
            return Ok(Lines::Done);
        };
        let mut line = trim_ascii(&buf);
        if first {
            line = line.strip_prefix(BOM).map(trim_ascii).unwrap_or(line);
        }
        if !complete {
            tally.records += 1;
            tally.unreadable += 1;
            *overlong_first |= first;
            first = false;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        tally.records += 1;
        // Cheap skip: a wrapped record of a kind we don't want is passed
        // over without parsing it.
        if let Some(k) = wrapped_kind(line) {
            if !want.contains(&k) {
                first = false;
                continue;
            }
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(v) => {
                first = false;
                if !hand_on(v, hint, want, tally, visit) {
                    return Ok(Lines::Stopped);
                }
            }
            Err(_) if first && first_may_be_document => {
                tally.records -= 1;
                return Ok(Lines::NotNdjson(line.to_vec()));
            }
            Err(_) => {
                first = false;
                tally.unreadable += 1;
            }
        }
    }
}

/// Hands each wanted record in `v` to `visit`; false when it asked to stop.
fn hand_on(
    v: Value,
    hint: Option<Kind>,
    want: &[Kind],
    tally: &mut Tally,
    visit: &mut impl FnMut(Kind, Value) -> bool,
) -> bool {
    let found = records(v, hint);
    if found.is_empty() {
        tally.unrecognised += 1;
    }
    tally.recognised += found.len() as u64;
    for (k, record) in found {
        if want.contains(&k) && !visit(k, record) {
            return false;
        }
    }
    true
}

fn read_people(
    path: &Path,
) -> Result<(Vec<api::Organization>, Vec<api::User>, Tally), SourceError> {
    let mut orgs = Vec::new();
    let mut users = Vec::new();
    let mut bad = 0u64;
    let mut tally = each_record(path, &[Kind::User, Kind::Organization], LIMITS, |k, v| {
        match k {
            Kind::User => match serde_json::from_value(v) {
                Ok(u) => users.push(u),
                Err(_) => bad += 1,
            },
            Kind::Organization => match serde_json::from_value(v) {
                Ok(o) => orgs.push(o),
                Err(_) => bad += 1,
            },
            Kind::Ticket => {}
        }
        true
    })?;
    tally.unreadable += bad;
    Ok((orgs, users, tally))
}

/// Sends each ticket down `tx`; stops when the receiver has gone (the run
/// failed or was cancelled).
fn read_tickets(path: &Path, tx: &mpsc::Sender<api::Ticket>) -> Result<Tally, SourceError> {
    let mut bad = 0u64;
    let mut tally = each_record(
        path,
        &[Kind::Ticket],
        LIMITS,
        |_, v| match serde_json::from_value::<api::Ticket>(v) {
            Ok(t) => tx.blocking_send(t).is_ok(),
            Err(_) => {
                bad += 1;
                true
            }
        },
    )?;
    tally.unreadable += bad;
    Ok(tally)
}

/// Reads one line into `buf`, at most `cap` bytes of it. None at the end;
/// `Some(false)` for a line too long to keep (the rest of it is read and
/// dropped).
fn read_line_capped(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
    cap: u64,
) -> io::Result<Option<bool>> {
    let n = reader.by_ref().take(cap).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') || (n as u64) < cap {
        return Ok(Some(true));
    }
    let mut sink = Vec::new();
    loop {
        sink.clear();
        let m = reader
            .by_ref()
            .take(1024 * 1024)
            .read_until(b'\n', &mut sink)?;
        if m == 0 || sink.last() == Some(&b'\n') {
            break;
        }
    }
    buf.clear();
    Ok(Some(false))
}

fn trim_ascii(b: &[u8]) -> &[u8] {
    let start = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(start, |p| p + 1);
    &b[start..end]
}

fn kind_from_name(name: &str) -> Option<Kind> {
    let n = name.to_ascii_lowercase();
    if n.contains("ticket") {
        Some(Kind::Ticket)
    } else if n.contains("organization") || n.contains("organisation") {
        Some(Kind::Organization)
    } else if n.contains("user") {
        Some(Kind::User)
    } else {
        None
    }
}

/// The kind of a `{"ticket": ...}`-style line, from its first bytes.
fn wrapped_kind(line: &[u8]) -> Option<Kind> {
    let head = &line[..line.len().min(32)];
    let head: Vec<u8> = head
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    if head.starts_with(b"{\"ticket\":") {
        Some(Kind::Ticket)
    } else if head.starts_with(b"{\"user\":") {
        Some(Kind::User)
    } else if head.starts_with(b"{\"organization\":") {
        Some(Kind::Organization)
    } else {
        None
    }
}

/// The records in one parsed value: unwrapped from `{"ticket": {...}}`,
/// expanded from `{"tickets": [...]}` or a bare array, or a bare record
/// recognised by its fields.
fn records(v: Value, hint: Option<Kind>) -> Vec<(Kind, Value)> {
    const WRAPPERS: [(&str, Kind); 3] = [
        ("ticket", Kind::Ticket),
        ("user", Kind::User),
        ("organization", Kind::Organization),
    ];
    const LISTS: [(&str, Kind); 3] = [
        ("tickets", Kind::Ticket),
        ("users", Kind::User),
        ("organizations", Kind::Organization),
    ];
    match v {
        Value::Array(items) => items.into_iter().flat_map(|i| records(i, hint)).collect(),
        Value::Object(mut obj) => {
            for (key, kind) in WRAPPERS {
                if obj.len() == 1 && obj.get(key).is_some_and(Value::is_object) {
                    return obj.remove(key).map(|r| vec![(kind, r)]).unwrap_or_default();
                }
            }
            let mut listed = Vec::new();
            for (key, kind) in LISTS {
                if let Some(Value::Array(items)) = obj.remove(key) {
                    listed.extend(items.into_iter().map(|i| (kind, i)));
                }
            }
            if !listed.is_empty() {
                return listed;
            }
            let has = |k: &str| obj.contains_key(k);
            let kind = if has("subject") || has("requester_id") || has("comments") || has("status")
            {
                Some(Kind::Ticket)
            } else if has("role") || has("email") {
                Some(Kind::User)
            } else if has("domain_names") {
                Some(Kind::Organization)
            } else {
                hint
            };
            match kind {
                Some(k) if obj.contains_key("id") => vec![(k, Value::Object(obj))],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn records_unwrap_expand_and_recognise_shapes() {
        let r = records(json!({"ticket": {"id": 1}}), None);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, Kind::Ticket);

        let r = records(json!({"users": [{"id": 1}, {"id": 2}]}), None);
        assert!(r.iter().all(|(k, _)| *k == Kind::User));
        assert_eq!(r.len(), 2);

        assert_eq!(
            records(json!({"id": 3, "role": "agent"}), None)[0].0,
            Kind::User
        );
        assert_eq!(
            records(json!({"id": 4, "name": "Acme"}), Some(Kind::Organization))[0].0,
            Kind::Organization
        );
        assert!(records(json!({"id": 4, "name": "Acme"}), None).is_empty());
    }

    fn zip_of(dir: &Path, body: &[u8]) -> std::path::PathBuf {
        use std::io::Write;
        let path = dir.join("e.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("zip"));
        zip.start_file("export_1", zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(body).expect("body");
        zip.finish().expect("finish");
        path
    }

    #[test]
    fn a_record_over_the_limits_fails_rather_than_vanish() {
        let small = Limits {
            line: 64,
            document: 128,
        };
        let dir = tempfile::tempdir().expect("dir");
        // One document over many lines, larger than the document limit.
        let doc = serde_json::to_vec_pretty(&json!({"tickets": [
            {"id": 1, "subject": "x".repeat(40)}, {"id": 2, "subject": "y".repeat(40)},
            {"id": 3, "subject": "z".repeat(40)}
        ]}))
        .expect("doc");
        let err = each_record(&zip_of(dir.path(), &doc), &[Kind::Ticket], small, |_, _| {
            true
        })
        .expect_err("over the document limit");
        assert!(matches!(err, SourceError::Protocol(_)), "{err:?}");
        // One line longer than the line limit, and nothing else.
        let line = format!(
            "{}\n",
            json!({"ticket": {"id": 1, "subject": "q".repeat(100)}})
        );
        let dir = tempfile::tempdir().expect("dir");
        assert!(each_record(
            &zip_of(dir.path(), line.as_bytes()),
            &[Kind::Ticket],
            small,
            |_, _| true
        )
        .is_err());
        // Within the limits, the same document reads.
        // A pretty-printed document whose lines include bare scalars, read
        // line by line, still yields no record: it is refused too.
        let scalars = serde_json::to_vec_pretty(&json!({"tickets": [
            {"id": 1, "tags": ["vip", "x".repeat(60)]}, {"id": 2, "ids": [1, 2, 3]}
        ]}))
        .expect("doc");
        let dir = tempfile::tempdir().expect("dir");
        assert!(each_record(
            &zip_of(dir.path(), &scalars),
            &[Kind::Ticket],
            small,
            |_, _| true
        )
        .is_err());
        let roomy = Limits {
            line: 64,
            document: 4096,
        };
        let dir = tempfile::tempdir().expect("dir");
        let mut n = 0;
        each_record(&zip_of(dir.path(), &doc), &[Kind::Ticket], roomy, |_, _| {
            n += 1;
            true
        })
        .expect("read");
        assert_eq!(n, 3);
    }

    #[test]
    fn wrapped_kind_reads_the_line_head() {
        assert_eq!(wrapped_kind(br#"{"ticket":{"id":1}}"#), Some(Kind::Ticket));
        assert_eq!(wrapped_kind(br#"{ "user" : {"id":1}}"#), Some(Kind::User));
        assert_eq!(wrapped_kind(br#"{"id":1}"#), None);
    }
}
