//! Lists that live in more than one layer agree. Priorities: the Rust enum,
//! the database enum and the app's `PRIORITY_OPTIONS` name the same five
//! values, and the app lists them most severe first, the reverse of
//! `TicketPriority::ALL`.

use diesel::prelude::*;
use diesel::sql_types::Text;
use regex::Regex;

use backend::models::TicketPriority;

use crate::common;

#[test]
fn priorities_agree_across_layers() {
    let rust: Vec<&str> = TicketPriority::ALL.iter().map(|p| p.as_str()).collect();

    #[derive(QueryableByName)]
    struct Label {
        #[diesel(sql_type = Text)]
        label: String,
    }
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let mut database: Vec<String> =
        diesel::sql_query("SELECT unnest(enum_range(NULL::ticket_priority))::text AS label")
            .load::<Label>(&mut conn)
            .expect("enum labels")
            .into_iter()
            .map(|l| l.label)
            .collect();
    database.sort();
    let mut sorted = rust.clone();
    sorted.sort();
    assert_eq!(database, sorted, "the database enum");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packages/core/src/constants/ticketOptions.ts");
    let ts = std::fs::read_to_string(&path).expect("read ticketOptions.ts");
    let start = ts.find("PRIORITY_OPTIONS").expect("PRIORITY_OPTIONS");
    let body = &ts[start..start + ts[start..].find("];").expect("end of the list")];
    let app: Vec<String> = Regex::new(r#"value:\s*["'](\w+)["']"#)
        .expect("pattern")
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect();
    let mut most_severe_first = rust.clone();
    most_severe_first.reverse();
    assert_eq!(
        app, most_severe_first,
        "PRIORITY_OPTIONS, most severe first"
    );
}
