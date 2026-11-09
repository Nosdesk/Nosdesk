//! Lint: writes to `assets`, `users`, `workspace_members` and `comments` stay
//! in each table's owning repository file.
//!
//! The owners' writers send the sync event for the row they change, so every
//! open client hears of it. A write anywhere else changes a synced row that no
//! event names: a stock ledger setting `assets.quantity`, the thumbnail job
//! setting `users.avatar_thumb`, a membership change leaving the user row's
//! `workspace_role` stale in every pool. `ticket_write_lint` holds `tickets` to
//! the same rule. This finds, outside a table's owners:
//!
//! - Diesel writes whose target is a path ending in the table (`users::table`,
//!   `u::users` through a `dsl as u` alias, `crate::schema::comments::table`),
//!   across line breaks, and through a module alias (`use
//!   crate::schema::assets as a;` then `update(a::table)`);
//! - writes through a query bound from the table first (`let q =
//!   assets::table...;` then `delete(q)`);
//! - SQL that inserts into, updates or deletes from the table, in any case,
//!   with `ONLY`, a schema or an alias.
//!
//! Test code doesn't count: `#[cfg(test)]` items and modules, including a
//! module declared `#[cfg(test)] mod name;` in its own file.
//!
//! Out of its reach: the restores and resets that write every table
//! generically, with the table name in a variable (`services/backup.rs`,
//! `services/workspace_import.rs`, `services/workspace_archive`), and rows the
//! database changes on its own through a foreign key's `ON DELETE`.
//!
//! ## Escape hatch
//!
//! A file that writes one of these tables for a reason of its own goes in
//! `ALLOWLIST` with the table and the reason. An entry whose file no longer
//! writes that table fails the lint, so the list can't outlive its reasons.

#![allow(clippy::expect_used)]

use std::collections::HashSet;

use regex::Regex;

use crate::lint_source::{mask, rust_sources, test_only_files, test_regions};

/// Each table and the files that own its writes.
const OWNERS: &[(&str, &[&str])] = &[
    ("assets", &["repository/assets.rs"]),
    (
        "users",
        &["repository/users.rs", "repository/user_helpers.rs"],
    ),
    ("workspace_members", &["repository/workspaces.rs"]),
    ("comments", &["repository/comments.rs"]),
];

/// Writes outside a table's owners, as (file, table, reason).
const ALLOWLIST: &[(&str, &str, &str)] = &[
    (
        "repository/feature_flags.rs",
        "users",
        "sets a person's `feature_flag_overrides`, which the synced user row doesn't carry",
    ),
    (
        "repository/user_helpers.rs",
        "workspace_members",
        "`create_user_with_email` grants the new account its first membership, and the \
         `user.created` it sends carries that role",
    ),
    (
        "repository/users.rs",
        "workspace_members",
        "`set_user_roles` sets the workspace role beside the platform role and sends \
         `user.updated` with both",
    ),
    (
        "repository/workspace_import_jobs.rs",
        "workspace_members",
        "a reset removes the memberships its import created, in the transaction that \
         bumps the workspace's data epoch, so every client starts over",
    ),
    (
        "repository/ticket_merge.rs",
        "comments",
        "the merge marker is inserted without `create_comment` so it doesn't stamp \
         `first_response_at`, and the merge sends its `comment.created`",
    ),
    (
        "repository/tickets.rs",
        "comments",
        "deleting a ticket deletes its replies with it, under the one `ticket.deleted`",
    ),
    (
        "bin/seed_demo.rs",
        "comments",
        "demo data, backdated by hand",
    ),
];

#[test]
fn writes_to_owned_tables_stay_with_their_owners() {
    let sources = rust_sources();
    let test_files = test_only_files(&sources);
    let known: HashSet<&str> = sources.iter().map(|(rel, _)| rel.as_str()).collect();

    let missing: Vec<&str> = OWNERS
        .iter()
        .flat_map(|(_, owners)| owners.iter().copied())
        .filter(|owner| !known.contains(owner))
        .collect();
    assert!(
        missing.is_empty(),
        "OWNERS names files that don't exist; point it at the table's owner: {missing:?}"
    );

    let mut stray = Vec::new();
    let mut allowlisted_seen = HashSet::new();
    for (rel, src) in &sources {
        if rel == "schema.rs" || test_files.contains(rel) {
            continue;
        }
        for &(table, owners) in OWNERS {
            if owners.contains(&rel.as_str()) {
                continue;
            }
            let writes = table_writes(src, table);
            if writes.is_empty() {
                continue;
            }
            if ALLOWLIST.iter().any(|(f, t, _)| f == rel && *t == table) {
                allowlisted_seen.insert((rel.clone(), table));
                continue;
            }
            for (line, text) in writes {
                stray.push(format!("{rel}:{line}: {table}: {text}"));
            }
        }
    }

    assert!(
        stray.is_empty(),
        "writes to an owned table outside its owner; call the owner's writer, which sends \
         the row's sync event, or add (file, table, reason) to ALLOWLIST:\n  {}",
        stray.join("\n  ")
    );
    let stale: Vec<(&str, &str)> = ALLOWLIST
        .iter()
        .map(|&(file, table, _)| (file, table))
        .filter(|&(file, table)| !allowlisted_seen.contains(&(file.to_string(), table)))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWLIST names files that no longer write that table; remove them: {stale:?}"
    );
}

/// Each spelling the lint must catch, and the code it must not.
#[test]
fn the_lint_sees_every_spelling_and_skips_test_code() {
    let caught = [
        (
            "assets",
            r#"diesel::sql_query("UPDATE assets SET quantity = $1 WHERE id = $2")"#,
        ),
        (
            "assets",
            "diesel::update(assets::table.find(id)).set(x).execute(conn)",
        ),
        (
            "assets",
            "diesel::update(\n        crate::schema::assets::table\n            .find(id))",
        ),
        (
            "assets",
            "use crate::schema::assets as a;\ndiesel::update(a::table.find(id))",
        ),
        (
            "assets",
            "let q = assets::table.filter(f);\n    diesel::delete(q).execute(c)",
        ),
        ("users", "diesel::update(u::users.filter(u::uuid.eq(id)))"),
        (
            "users",
            "diesel::insert_into(crate::schema::users::table).values(&u)",
        ),
        (
            "users",
            r#"sql_query("update public.users as u set avatar_thumb = $1")"#,
        ),
        (
            "workspace_members",
            r#"sql_query("INSERT INTO workspace_members (user_uuid) VALUES ($1)")"#,
        ),
        (
            "workspace_members",
            "diesel::delete(\n    m::workspace_members\n        .filter(f))",
        ),
        (
            "comments",
            "diesel::insert_into(crate::schema::comments::table).values(&c)",
        ),
        (
            "comments",
            r#"sql_query("DELETE FROM ONLY comments WHERE id = $1")"#,
        ),
    ];
    for (table, code) in caught {
        assert_eq!(table_writes(code, table).len(), 1, "should catch: {code}");
    }
    let ignored = [
        ("assets", "assets::table.find(id).first(conn)"),
        ("assets", "diesel::update(ticket_assets::table)"),
        ("assets", "diesel::update(asset_loans::table.find(id))"),
        ("assets", "let a = crate::repository::assets::get_device_by_id(c, id)?;\ndiesel::delete(a)"),
        ("users", "diesel::update(user_emails::table)"),
        ("users", "users_repo::update_user(&id, change, conn, None)"),
        ("users", "// diesel::update(users::table) in a comment"),
        ("users", r#"warn!("failed to update users for {id}")"#),
        ("comments", "/* DELETE FROM comments */ let a = 1;"),
        ("comments", "#[cfg(test)]\nmod tests {\n    fn f() { diesel::delete(comments::table); }\n}"),
        ("workspace_members", "#[cfg(test)]\nfn helper(c: &mut Conn) {\n    diesel::update(workspace_members::table);\n}"),
    ];
    for (table, code) in ignored {
        assert!(
            table_writes(code, table).is_empty(),
            "should ignore: {code}"
        );
    }
}

/// The writes to `table` in `src` outside test code, as (line, text).
fn table_writes(src: &str, table: &str) -> Vec<(usize, String)> {
    let code = mask(src, true);
    let sql = mask(src, false);
    let skipped = test_regions(&code);
    let t = regex::escape(table);
    // A write whose target is a path ending in the table.
    let diesel = Regex::new(&format!(
        r"\b(?:insert_into|update|delete)\s*\(\s*(?:\w+\s*::\s*)*{t}\b"
    ))
    .expect("diesel write pattern");
    // A query bound from the table (`table::table...` or a dsl's `t::table.`),
    // written through later.
    let bound = Regex::new(&format!(
        r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=\s*(?:\w+\s*::\s*)*{t}\s*(?:::\s*table\b|\.)"
    ))
    .expect("binding pattern");
    // A module alias (`use crate::schema::T as a;`, `T::dsl as a`) names the
    // table only at its `use`.
    let alias =
        Regex::new(&format!(r"\b{t}(?:\s*::\s*dsl)?\s+as\s+(\w+)\b")).expect("alias pattern");
    let through: Vec<Regex> = bound
        .captures_iter(&code)
        .map(|caps| {
            format!(
                r"\b(?:update|delete)\s*\(\s*&?\s*{}\b",
                regex::escape(&caps[1])
            )
        })
        .chain(alias.captures_iter(&code).map(|caps| {
            format!(
                r"\b(?:insert_into|update|delete)\s*\(\s*{}\s*::\s*(?:table|{t})\b",
                regex::escape(&caps[1])
            )
        }))
        .map(|pattern| Regex::new(&pattern).expect("write-through pattern"))
        .collect();
    let raw = Regex::new(&format!(
        r#"(?i)\b(?:update\s+(?:only\s+)?(?:public\.)?"?{t}"?(?:\s+(?:as\s+)?\w+)?\s+set|insert\s+into\s+(?:public\.)?"?{t}"?\b|delete\s+from\s+(?:only\s+)?(?:public\.)?"?{t}"?\b)"#
    ))
    .expect("sql write pattern");
    let mut found: Vec<(usize, String)> = diesel
        .find_iter(&code)
        .chain(through.iter().flat_map(|re| re.find_iter(&code)))
        .chain(raw.find_iter(&sql))
        .filter(|m| !skipped.iter().any(|r| r.contains(&m.start())))
        .map(|m| {
            let line = src[..m.start()].matches('\n').count() + 1;
            let text = src[m.range()]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (line, text)
        })
        .collect();
    found.sort();
    found.dedup();
    found
}
