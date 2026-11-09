//! Lint: attachment sync events are built in one place, and only the
//! attachment repository writes the `attachments` table.
//!
//! Who receives an attachment's events depends on the row: a file on a reply
//! goes to its ticket's audience, a draft to its uploader, a guest's draft to
//! no one. When creates, claims and deletes each picked their own audience,
//! guest drafts reached the whole workspace and fired webhooks. Every event
//! now comes from `repository::comments::attachment_event`, and this keeps a
//! new writer from choosing its own.
//!
//! Writes are found the way `table_owner_write_lint` finds them
//! (`lint_source::table_writes`), so any path to the table counts
//! (`attachments::table`, `crate::schema::attachments::table`, a `dsl` or
//! module alias, a query bound first) as does raw SQL. Test code doesn't count.
//!
//! ## Escape hatch
//!
//! A write to `attachments` outside `repository/comments.rs` goes in
//! `WRITE_ALLOWLIST` with a one-line reason. An entry whose file no longer
//! writes the table fails the lint.

#![allow(clippy::expect_used)]

use regex::Regex;

use crate::lint_source::{mask, rust_sources, table_writes, test_only_files};

const EMITTER_FILE: &str = "repository/comments.rs";
const EMITTER_FN: &str = "fn attachment_event(";

/// Files outside the attachment repository that may write `attachments`, with
/// the reason.
const WRITE_ALLOWLIST: &[(&str, &str)] = &[
    (
        "repository/users.rs",
        "account deletion clears `uploaded_by`, which no sync event carries",
    ),
    (
        "repository/tickets.rs",
        "deleting a ticket deletes its replies' files with them, under the one \
         `ticket.deleted`, as `table_owner_write_lint` allows for the replies",
    ),
];

#[test]
fn attachment_events_come_from_one_builder() {
    let sources = rust_sources();
    let test_files = test_only_files(&sources);
    let event_re = Regex::new(r"aggregate:\s*SyncAggregate::Attachment\b").expect("event pattern");

    let mut stray_events = Vec::new();
    let mut stray_writes = Vec::new();
    let mut allowlisted_seen = Vec::new();
    let mut builder_events = 0;

    for (relpath, src) in &sources {
        if relpath == "schema.rs" || test_files.contains(relpath) {
            continue;
        }
        let code = mask(src, true);

        // The builder's body runs from its signature to the next item.
        let builder = (relpath == EMITTER_FILE)
            .then(|| code.find(EMITTER_FN))
            .flatten()
            .map(|start| {
                let rest = &code[start + EMITTER_FN.len()..];
                let end = rest.find("\nfn ").or_else(|| rest.find("\npub fn "));
                start..end.map_or(code.len(), |e| start + EMITTER_FN.len() + e)
            });
        for m in event_re.find_iter(&code) {
            if builder.as_ref().is_some_and(|b| b.contains(&m.start())) {
                builder_events += 1;
            } else {
                stray_events.push(format!("{relpath} (byte {})", m.start()));
            }
        }

        if relpath == EMITTER_FILE {
            continue;
        }
        let writes = table_writes(src, "attachments");
        if writes.is_empty() {
            continue;
        }
        if WRITE_ALLOWLIST.iter().any(|(f, _)| f == relpath) {
            allowlisted_seen.push(relpath.clone());
            continue;
        }
        for (line, text) in writes {
            stray_writes.push(format!("{relpath}:{line}: {text}"));
        }
    }

    assert_eq!(
        builder_events, 1,
        "expected exactly one attachment event in {EMITTER_FILE}::attachment_event"
    );
    assert!(
        stray_events.is_empty(),
        "attachment sync events built outside attachment_event; build them there so the \
         audience follows the row:\n  {}",
        stray_events.join("\n  ")
    );
    assert!(
        stray_writes.is_empty(),
        "writes to `attachments` outside {EMITTER_FILE}; go through its functions, which \
         emit, or add the file to WRITE_ALLOWLIST with a reason:\n  {}",
        stray_writes.join("\n  ")
    );
    let stale: Vec<&str> = WRITE_ALLOWLIST
        .iter()
        .map(|(file, _)| *file)
        .filter(|file| !allowlisted_seen.iter().any(|seen| seen == file))
        .collect();
    assert!(
        stale.is_empty(),
        "WRITE_ALLOWLIST names files that no longer write `attachments`; remove them: {stale:?}"
    );
}

/// The spellings of a write to `attachments` the lint must catch, and what it
/// must leave alone.
#[test]
fn the_lint_sees_every_attachments_spelling() {
    let caught = [
        "diesel::delete(attachments::table.find(id)).execute(conn)",
        "diesel::delete(crate::schema::attachments::table.find(attachment.id)).execute(conn)",
        "diesel::update(\n    crate::schema::attachments::table\n        .filter(f))",
        "use crate::schema::attachments::dsl as a;\ndiesel::delete(a::attachments.find(id))",
        "let q = attachments::table.filter(f);\n    diesel::delete(q).execute(c)",
        r#"diesel::sql_query("DELETE FROM attachments WHERE id = $1")"#,
    ];
    for code in caught {
        assert_eq!(
            table_writes(code, "attachments").len(),
            1,
            "should catch: {code}"
        );
    }
    let ignored = [
        "attachments::table.find(id).first(conn)",
        "use crate::schema::attachments::dsl as a;\nlet n: i32 = a::attachments.count().get_result(c)?;",
        "diesel::delete(ticket_attachments::table)",
        "// diesel::delete(attachments::table) in a comment",
        "#[cfg(test)]\nmod tests {\n    fn f() { diesel::delete(attachments::table); }\n}",
    ];
    for code in ignored {
        assert!(
            table_writes(code, "attachments").is_empty(),
            "should ignore: {code}"
        );
    }
}
