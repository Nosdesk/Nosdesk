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
//!   `{"tickets": [...]}`) is read whole, up to a size limit;
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
    Body, BodyFormat, Bundle, Comment, FetchState, Manifest,
};
use crate::services::imports::helpdesk::source::{FetchProgress, SourceError};

/// A line longer than this is not a record we can carry; it is skipped and
/// counted. Zendesk keeps ticket records under about 1 MB.
const MAX_LINE: u64 = 64 * 1024 * 1024;
/// The most read of an entry that is one JSON document rather than NDJSON.
const MAX_DOCUMENT: u64 = 256 * 1024 * 1024;
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
    let (orgs, users, unreadable) = tokio::task::spawn_blocking(move || read_people(&path))
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
            .assemble(bundle, &t, created_at, &fields, &raw, &cancel)
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
    let skipped_lines = reader
        .await
        .map_err(|e| SourceError::Protocol(format!("export reader stopped: {e}")))??;
    state.tickets_len = writer.checkpoint()?;
    state.tickets_done = true;
    run.report(source);

    let unreadable = unreadable + skipped_lines;
    if unreadable > 0 {
        notes.push(format!(
            "{unreadable} lines of the export could not be read and were skipped"
        ));
    }
    state.counts = run.counts.clone();
    state.notes = notes;
    bundle.write_state(&state)?;
    let manifest = run.manifest("zendesk", &source.scope, &state.notes);
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
            value: text.clone(),
        },
        created_at,
        attachments: vec![],
    })
}

fn zip_error(e: zip::result::ZipError) -> SourceError {
    SourceError::Protocol(format!("export zip: {e}"))
}

/// Visits every record of every entry in the zip. `want` says which kinds
/// the caller needs; the rest are passed over without being decoded.
/// Returns the number of lines that couldn't be read.
fn each_record(
    path: &Path,
    want: &[Kind],
    mut visit: impl FnMut(Kind, Value) -> bool,
) -> Result<u64, SourceError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    let mut unreadable = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(zip_error)?;
        if entry.is_dir() {
            continue;
        }
        let hint = entry.name().map(|n| kind_from_name(&n)).unwrap_or(None);
        let mut reader = BufReader::new(entry);
        let mut first = true;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let Some(complete) = read_line_capped(&mut reader, &mut buf)? else {
                break;
            };
            if !complete {
                unreadable += 1;
                first = false;
                continue;
            }
            let line = trim_ascii(&buf);
            if line.is_empty() {
                continue;
            }
            // Cheap skip: a wrapped record of a kind we don't want is
            // passed over without parsing it.
            if let Some(k) = wrapped_kind(line) {
                if !want.contains(&k) {
                    first = false;
                    continue;
                }
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(v) => {
                    first = false;
                    for (k, record) in records(v, hint) {
                        if want.contains(&k) && !visit(k, record) {
                            return Ok(unreadable);
                        }
                    }
                }
                Err(_) if first => {
                    // Not NDJSON: perhaps one JSON document over many lines.
                    let mut doc = line.to_vec();
                    doc.push(b'\n');
                    reader.by_ref().take(MAX_DOCUMENT).read_to_end(&mut doc)?;
                    match serde_json::from_slice::<Value>(&doc) {
                        Ok(v) => {
                            for (k, record) in records(v, hint) {
                                if want.contains(&k) && !visit(k, record) {
                                    return Ok(unreadable);
                                }
                            }
                        }
                        Err(_) => unreadable += 1,
                    }
                    break;
                }
                Err(_) => unreadable += 1,
            }
        }
    }
    Ok(unreadable)
}

fn read_people(path: &Path) -> Result<(Vec<api::Organization>, Vec<api::User>, u64), SourceError> {
    let mut orgs = Vec::new();
    let mut users = Vec::new();
    let mut bad = 0u64;
    let unreadable = each_record(path, &[Kind::User, Kind::Organization], |k, v| {
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
    Ok((orgs, users, unreadable + bad))
}

/// Sends each ticket down `tx`; stops when the receiver has gone (the run
/// failed or was cancelled).
fn read_tickets(path: &Path, tx: &mpsc::Sender<api::Ticket>) -> Result<u64, SourceError> {
    let mut bad = 0u64;
    let unreadable = each_record(path, &[Kind::Ticket], |_, v| {
        match serde_json::from_value::<api::Ticket>(v) {
            Ok(t) => tx.blocking_send(t).is_ok(),
            Err(_) => {
                bad += 1;
                true
            }
        }
    })?;
    Ok(unreadable + bad)
}

/// Reads one line into `buf`, at most [`MAX_LINE`] bytes of it. None at
/// the end; `Some(false)` for a line too long to keep (the rest of it is
/// read and dropped).
fn read_line_capped(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> io::Result<Option<bool>> {
    let n = reader.by_ref().take(MAX_LINE).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') || (n as u64) < MAX_LINE {
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

    #[test]
    fn wrapped_kind_reads_the_line_head() {
        assert_eq!(wrapped_kind(br#"{"ticket":{"id":1}}"#), Some(Kind::Ticket));
        assert_eq!(wrapped_kind(br#"{ "user" : {"id":1}}"#), Some(Kind::User));
        assert_eq!(wrapped_kind(br#"{"id":1}"#), None);
    }
}
