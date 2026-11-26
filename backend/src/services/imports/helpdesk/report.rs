//! Plain-text renderings of a plan and a run, for the operator CLI and the
//! report file a run leaves beside its bundle.

use std::fmt::Write;

use super::loader::{counted, LoadReport, MissingPeople, PeopleDetail, Plan};
use super::people::{source_title, ProfileTally};

pub fn render_plan(plan: &Plan) -> String {
    let mut s = String::new();
    let title = source_title(&plan.source);
    let title = title.as_str();
    let _ = writeln!(s, "Source: {} ({})", plan.source, plan.scope);
    if let Some(inputs) = &plan.inputs {
        let mut from = inputs.names().join(", ");
        if from.is_empty() {
            from = format!("the {title} API");
        } else if inputs.api_fill {
            from.push_str(" (+ API fill)");
        }
        let _ = writeln!(s, "Read from: {from}");
    }
    let _ = writeln!(
        s,
        "Tickets: {} ({} imported already, {} to write)",
        plan.tickets,
        plan.tickets_imported_already,
        plan.tickets - plan.tickets_imported_already
    );
    let _ = writeln!(s, "Replies: {}", plan.comments);
    let _ = writeln!(
        s,
        "Files: {} ({} the bundle doesn't have)",
        plan.attachments, plan.attachments_missing
    );
    let _ = writeln!(s, "Ticket numbers: {}", plan.numbers_reason);
    let _ = writeln!(s, "SLA: off on imported tickets; switch it on per ticket.");
    let _ = writeln!(
        s,
        "Notifications, rules, webhooks: none fire for imported history."
    );

    let _ = writeln!(s, "\nStatuses:");
    for (from, (to, n)) in &plan.states {
        let _ = writeln!(s, "  {from:<20} -> {to}  ({n})");
    }
    if !plan.types.is_empty() {
        let _ = writeln!(s, "\nTypes:");
        for (from, (to, n)) in &plan.types {
            let to = to
                .as_deref()
                .map_or("a tag (no request type matches)".to_string(), |t| {
                    format!("request type \"{t}\"")
                });
            let _ = writeln!(s, "  {from:<20} -> {to}  ({n})");
        }
    }
    let d = &plan.people_detail;
    let _ = writeln!(s, "\nPeople the tickets mention: {}", d.mentioned);
    for (outcome, n) in &plan.people {
        let _ = writeln!(s, "  {outcome:<40} {n}");
    }
    missing_people(&mut s, plan.missing_people.as_ref(), 40);
    if let Some(g) = &plan.gone_people {
        let _ = writeln!(
            s,
            "  Gone at {title} (asked, not returned): {}, on {} and {};\n    credited to the placeholder, named in the note",
            g.people,
            counted(g.tickets, "ticket", "tickets"),
            counted(g.replies, "reply", "replies"),
        );
    }
    people_lines(&mut s, d, title);
    if !plan.staff.is_empty() {
        let _ = writeln!(s, "\nSource staff:");
        for (who, target, outcome) in &plan.staff {
            let _ = writeln!(s, "  {who:<40} -> {target}  ({outcome})");
        }
    }
    organization_lines(&mut s, d, title);
    if !d.changed_at_source.is_empty() {
        let _ = writeln!(
            s,
            "Changed at {title} since the last import: {} (not applied)",
            counted(d.changed_at_source.len() as u64, "person", "people")
        );
    }
    let _ = writeln!(
        s,
        "Not read: other addresses, user fields, notes, tags, photos, external ids, locale"
    );
    if let Some(f) = &plan.fill {
        let _ = writeln!(
            s,
            "Filled from the API: {}, {}, {} over 1 MB",
            counted(f.people_returned, "person", "people"),
            counted(f.organizations, "organization", "organizations"),
            counted(f.conversations, "conversation", "conversations"),
        );
        if let Some(why) = &f.failed {
            let _ = writeln!(s, "  The fill stopped: {why}");
        }
    }
    if !plan.notes.is_empty() {
        let _ = writeln!(s, "\nNotes:");
        for n in &plan.notes {
            let _ = writeln!(s, "  - {n}");
        }
    }
    let _ = writeln!(
        s,
        "\nEstimated time to write: about {} min",
        plan.estimated_minutes
    );
    s
}

/// A run's report, for the operator and the file left beside the bundle.
/// It names tickets and files, never a download link (which can carry a
/// token): the connector keeps those out of every reason it records.
pub fn render_report(r: &LoadReport) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Tickets written: {}", r.tickets_written);
    let _ = writeln!(s, "Tickets already there: {}", r.tickets_skipped);
    let _ = writeln!(
        s,
        "Tickets brought up to date from the source: {}",
        r.tickets_updated
    );
    if r.tickets_not_attempted > 0 {
        let _ = writeln!(
            s,
            "Tickets not reached before the run stopped: {}",
            r.tickets_not_attempted
        );
    }
    let _ = writeln!(s, "Replies written: {}", r.comments_written);
    let _ = writeln!(s, "Files attached: {}", r.attachments_written);
    if !r.people.is_empty() || r.missing_people.is_some() || r.gone_people.is_some() {
        let _ = writeln!(s, "People:");
        for (outcome, n) in &r.people {
            let _ = writeln!(s, "  {outcome:<40} {n}");
        }
        missing_people(&mut s, r.missing_people.as_ref(), 40);
        missing_people(&mut s, r.gone_people.as_ref(), 40);
    }
    profile_lines(&mut s, &r.profile);
    list(
        &mut s,
        "People not brought across as planned",
        &r.people_problems,
    );
    list(
        &mut s,
        "People whose organization or phone couldn't be written",
        &r.profile_problems,
    );
    list(
        &mut s,
        "People changed at the source since they were imported, not applied",
        &r.changed_at_source,
    );
    list(
        &mut s,
        "Tickets that couldn't be written (the next run tries them again)",
        &r.tickets_failed,
    );
    list(
        &mut s,
        "Tickets changed here since they were imported, left as they are",
        &r.tickets_changed_here,
    );
    list(
        &mut s,
        "Tickets numbered after existing ones",
        &r.numbers_moved,
    );
    list(&mut s, "Tickets not brought across again", &r.ticket_notes);
    list(&mut s, "Files not brought across", &r.attachment_problems);
    if r.unreadable_lines > 0 {
        let _ = writeln!(
            s,
            "Lines of the bundle that couldn't be read, skipped: {}",
            r.unreadable_lines
        );
    }
    s
}

/// What the plan says about the people beyond their outcomes.
fn people_lines(s: &mut String, d: &PeopleDetail, title: &str) {
    if d.deleted > 0 {
        let _ = writeln!(
            s,
            "  Deleted at {title}: {}, history kept under their name, no address stored",
            d.deleted
        );
    }
    if d.shared_addresses > 0 {
        let _ = writeln!(
            s,
            "  Shared addresses: {} used by {} {title} users, one account each",
            counted(d.shared_addresses, "address", "addresses"),
            d.shared_address_people
        );
    }
    if d.limited_staff > 0 {
        let _ = writeln!(
            s,
            "  Limited staff (light agents, chat only, contributors): {}, no seat unless mapped",
            d.limited_staff
        );
    }
    if !d.custom_role_agents.is_empty() {
        let _ = writeln!(
            s,
            "  Custom-role agents: {} (check with --map-agent): {}",
            d.custom_role_agents.len(),
            d.custom_role_agents.join(", ")
        );
    }
}

/// Organizations, phones and what portal sharing would show.
fn organization_lines(s: &mut String, d: &PeopleDetail, title: &str) {
    let p = &d.profile;
    let _ = writeln!(
        s,
        "\nOrganizations with someone brought across: {}",
        d.organizations
    );
    let _ = writeln!(
        s,
        "  As the person's organization: {} (already set here: {}; directory-synced: {})",
        p.organizations, p.organizations_kept, p.directory_synced
    );
    if d.organizations_missing > 0 {
        let _ = writeln!(
            s,
            "  Named but not in the bundle, so no organization: {}",
            counted(d.organizations_missing, "person", "people")
        );
    }
    let domains = if d.organizations_with_domains > 0 {
        format!(
            "domain names ({}), ",
            counted(
                d.organizations_with_domains,
                "organization",
                "organizations"
            )
        )
    } else {
        String::new()
    };
    let _ = writeln!(s, "  Not carried: {domains}notes, details, fields, tags");
    if let Some(sh) = &d.sharing {
        let at = format!(
            "At {} non-free-mail {}, {} from organizations that didn't share at {title}",
            sh.domains,
            if sh.domains == 1 { "domain" } else { "domains" },
            counted(sh.tickets, "ticket", "tickets"),
        );
        if sh.on {
            let _ = writeln!(
                s,
                "  Portal sharing by domain is on. {at} become visible to colleagues as each side verifies their address"
            );
        } else {
            let _ = writeln!(
                s,
                "  Portal sharing by domain is off. {at} would be shared if it were turned on"
            );
        }
    }
    let _ = writeln!(
        s,
        "Phones: {} (already one here: {}; shared numbers skipped: {})",
        p.phones, p.phones_kept, d.shared_phones
    );
}

/// The profile fill's counts, for the report.
fn profile_lines(s: &mut String, p: &ProfileTally) {
    if *p == ProfileTally::default() {
        return;
    }
    let _ = writeln!(
        s,
        "Organizations written: {} (already set here: {}; directory-synced: {})",
        p.organizations, p.organizations_kept, p.directory_synced
    );
    let _ = writeln!(
        s,
        "Phones added: {} (already one here: {})",
        p.phones, p.phones_kept
    );
}

/// The people the tickets name and the bundle lacks, as a line of the
/// people table and what it touches beneath.
fn missing_people(s: &mut String, missing: Option<&MissingPeople>, width: usize) {
    let Some(m) = missing else {
        return;
    };
    let outcome = format!("{} (placeholder)", m.label);
    let _ = writeln!(s, "  {outcome:<width$} {}", m.people);
    let _ = writeln!(
        s,
        "    on {} and {}; their requests and replies go to the placeholder author",
        counted(m.tickets, "ticket", "tickets"),
        counted(m.replies, "reply", "replies")
    );
}

fn list(s: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(s, "{title} ({}):", items.len());
    for item in items {
        let _ = writeln!(s, "  - {item}");
    }
}
