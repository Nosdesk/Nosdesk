//! Plain-text renderings of a plan and a run, for the operator CLI and the
//! report file a run leaves beside its bundle.

use std::fmt::Write;

use super::loader::{LoadReport, Plan};

pub fn render_plan(plan: &Plan) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Source: {} ({})", plan.source, plan.scope);
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
    let _ = writeln!(s, "\nPeople the tickets mention:");
    for (outcome, n) in &plan.people {
        let _ = writeln!(s, "  {outcome:<32} {n}");
    }
    if !plan.staff.is_empty() {
        let _ = writeln!(s, "\nSource staff:");
        for (who, target, outcome) in &plan.staff {
            let _ = writeln!(s, "  {who:<40} -> {target}  ({outcome})");
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
    let _ = writeln!(s, "Replies written: {}", r.comments_written);
    let _ = writeln!(s, "Files attached: {}", r.attachments_written);
    if !r.people.is_empty() {
        let _ = writeln!(s, "People:");
        for (outcome, n) in &r.people {
            let _ = writeln!(s, "  {outcome:<40} {n}");
        }
    }
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

fn list(s: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(s, "{title} ({}):", items.len());
    for item in items {
        let _ = writeln!(s, "  - {item}");
    }
}
