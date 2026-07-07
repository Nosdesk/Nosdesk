//! Search query execution

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, RegexQuery, TermQuery};
use tantivy::schema::IndexRecordOption;
use tantivy::{DocAddress, IndexReader, Order, TantivyDocument, Term};
use tracing::{debug, warn};

/// Minimum query-term length below which we skip prefix expansion
/// — single-character queries would otherwise match nearly every
/// term in the index and dominate scoring with noise.
const MIN_PREFIX_QUERY_LEN: usize = 2;

use super::schema::{fields, SearchSchema};
use super::types::{EntityType, SearchResponse, SearchResult, SortOrder};

/// Execute a search query against the index
///
/// `include_internal` controls whether `is_internal=1` documents
/// (internal-note comments) appear in the result set. Staff (Admin
/// / Technician) callers pass `true`; non-staff callers pass `false`
/// so they cannot reach internal notes through full-text search.
///
/// `workspace_id` is the caller's workspace. Every query is gated,
/// fail-closed, by a required workspace term so a caller can never
/// reach documents belonging to a workspace they're not in.
pub fn execute_search(
    reader: &IndexReader,
    schema: &SearchSchema,
    query_str: &str,
    limit: usize,
    entity_types: Option<&[EntityType]>,
    include_internal: bool,
    workspace_id: i64,
    sort: SortOrder,
) -> Result<SearchResponse, Box<dyn std::error::Error + Send + Sync>> {
    let start_time = std::time::Instant::now();

    let searcher = reader.searcher();

    // Build the query
    let query = build_search_query(
        schema,
        query_str,
        entity_types,
        include_internal,
        workspace_id,
    );

    // Execute the search. tantivy 0.26 split TopDocs from the Collector
    // trait; you pick a sort axis explicitly, and the two axes have
    // different fruit types, so each branch normalizes to
    // (score, DocAddress). For the Updated branch there is no BM25 score
    // — the ordering already reflects recency — so score is reported as 0.
    let top_docs: Vec<(f32, DocAddress)> = match sort {
        // order_by_score reproduces the previous default (BM25, descending).
        SortOrder::Relevance => {
            searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?
        }
        // Newest-first by the updated_at fast field. Docs missing the fast
        // value (shouldn't happen — every indexed doc writes updated_at)
        // sort last via a 0 timestamp.
        SortOrder::Updated => {
            let collector = TopDocs::with_limit(limit)
                .order_by_fast_field::<i64>(fields::UPDATED_AT, Order::Desc);
            searcher
                .search(&query, &collector)?
                .into_iter()
                .map(|(_ts, addr)| (0.0_f32, addr))
                .collect()
        }
    };

    let total = top_docs.len();

    // Convert results
    let results: Vec<SearchResult> = top_docs
        .into_iter()
        .filter_map(
            |(score, doc_address)| match searcher.doc::<TantivyDocument>(doc_address) {
                Ok(doc) => Some(document_to_result(&doc, schema, score)),
                Err(e) => {
                    warn!(error = ?e, "Failed to retrieve document");
                    None
                }
            },
        )
        .collect();

    let took_ms = start_time.elapsed().as_millis() as u64;

    debug!(
        query = query_str,
        results = results.len(),
        total = total,
        took_ms = took_ms,
        "Search completed"
    );

    Ok(SearchResponse {
        results,
        total,
        query: query_str.to_string(),
        took_ms,
    })
}

/// Build a Tantivy query from a search string
/// Uses term queries with field boosts for BM25 ranking
fn build_search_query(
    schema: &SearchSchema,
    query_str: &str,
    entity_types: Option<&[EntityType]>,
    include_internal: bool,
    workspace_id: i64,
) -> Box<dyn Query> {
    // Apply field boosts using BooleanQuery
    // Title gets 3x boost, content 1x, metadata 0.8x
    let title_query: Box<dyn Query> = Box::new(BoostQuery::new(
        Box::new(build_field_query(schema.title, query_str)),
        3.0,
    ));

    let content_query: Box<dyn Query> = Box::new(BoostQuery::new(
        Box::new(build_field_query(schema.content, query_str)),
        1.0,
    ));

    let metadata_query: Box<dyn Query> = Box::new(BoostQuery::new(
        Box::new(build_field_query(schema.metadata, query_str)),
        0.8,
    ));

    // The text match is a required group: a doc must match the query in
    // at least one of title / content / metadata. Wrapping it in its own
    // BooleanQuery (whose Should clauses keep min-should-match = 1 because
    // it has no Must clauses of its own) and adding THAT as an outer Must
    // keeps the text relevance mandatory even once the filter Must clauses
    // below (entity type, workspace) are present. A bare Should at the top
    // level would become optional the moment any Must is added, which would
    // return every doc passing the filters regardless of the query text.
    let text_query: Box<dyn Query> = Box::new(BooleanQuery::new(vec![
        (Occur::Should, title_query),
        (Occur::Should, content_query),
        (Occur::Should, metadata_query),
    ]));

    let mut subqueries: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, text_query)];

    // Add entity type filter if specified
    if let Some(types) = entity_types {
        if !types.is_empty() {
            let type_queries: Vec<(Occur, Box<dyn Query>)> = types
                .iter()
                .map(|t| {
                    let term = tantivy::Term::from_field_text(schema.entity_type, t.as_str());
                    let q: Box<dyn Query> =
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                    (Occur::Should, q)
                })
                .collect();

            let type_filter = BooleanQuery::new(type_queries);
            subqueries.push((Occur::Must, Box::new(type_filter)));
        }
    }

    // Visibility filter. Internal-note comments are indexed with
    // is_internal=1; non-staff callers ("user") get an explicit MustNot
    // clause so those documents drop out of the result set entirely. The
    // required text Must clause above is the positive branch the MustNot
    // scores against (a query that is only MustNot returns zero hits in
    // tantivy).
    if !include_internal {
        let internal_term = Term::from_field_i64(schema.is_internal, 1);
        let internal_q: Box<dyn Query> =
            Box::new(TermQuery::new(internal_term, IndexRecordOption::Basic));
        subqueries.push((Occur::MustNot, internal_q));
    }

    // Workspace tenancy gate. Every document carries one or more
    // workspace_id values; this required term restricts results to the
    // caller's workspace. It is added centrally here so no query path can
    // skip it: a document with no matching workspace value (including a
    // user with zero memberships) is unreachable (fail-closed).
    let workspace_term = Term::from_field_i64(schema.workspace_id, workspace_id);
    let workspace_q: Box<dyn Query> =
        Box::new(TermQuery::new(workspace_term, IndexRecordOption::Basic));
    subqueries.push((Occur::Must, workspace_q));

    Box::new(BooleanQuery::new(subqueries))
}

/// Build a query for a single field. Each whitespace-separated word
/// in the user query becomes a Should branch combining:
///   * an exact `TermQuery` (full-token match — wins on tf-idf when
///     the query happens to be the complete word, e.g. typing
///     "https" matches the indexed "https" token directly), and
///   * a `RegexQuery` anchored at the start of the term (matches any
///     indexed token starting with the typed prefix, e.g. "seb" →
///     "sebastian"). Tantivy's RegexQuery iterates the field's term
///     dictionary efficiently, so this is the lightweight stand-in
///     for "edge ngram at index time" without the index-size blowup
///     of indexing every prefix.
///
/// Words shorter than `MIN_PREFIX_QUERY_LEN` skip the prefix branch
/// to avoid matching nearly every term in the index.
fn build_field_query(field: tantivy::schema::Field, query_str: &str) -> BooleanQuery {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

    for raw_term in query_str.split_whitespace() {
        let term = raw_term.to_lowercase();
        if term.is_empty() {
            continue;
        }

        let tantivy_term = tantivy::Term::from_field_text(field, &term);
        let exact: Box<dyn Query> = Box::new(TermQuery::new(
            tantivy_term,
            IndexRecordOption::WithFreqsAndPositions,
        ));
        clauses.push((Occur::Should, exact));

        if term.chars().count() >= MIN_PREFIX_QUERY_LEN {
            let pattern = format!("{}.*", regex_escape(&term));
            if let Ok(regex_query) = RegexQuery::from_pattern(&pattern, field) {
                clauses.push((Occur::Should, Box::new(regex_query)));
            }
        }
    }

    BooleanQuery::new(clauses)
}

/// Escape regex metacharacters that may appear in user input so the
/// constructed pattern matches the literal characters (we add `.*`
/// ourselves to anchor a prefix match).
fn regex_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Convert a Tantivy document to a SearchResult
fn document_to_result(doc: &TantivyDocument, schema: &SearchSchema, score: f32) -> SearchResult {
    // tantivy 0.26: get_first returns CompactDocValue<'_>, whose typed
    // accessors live on the Value trait. Importing it brings as_str /
    // as_i64 / as_u64 into scope and replaces the previous OwnedValue
    // match dance.
    use tantivy::schema::Value;

    let get_text = |field: tantivy::schema::Field| -> String {
        doc.get_first(field)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    };

    let get_i64 = |field: tantivy::schema::Field| -> i64 {
        doc.get_first(field)
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
            .unwrap_or(0)
    };

    let id = get_text(schema.id);
    let entity_type = get_text(schema.entity_type);
    let entity_id = get_i64(schema.entity_id);
    let title = get_text(schema.title);
    let preview = get_text(schema.preview);
    let url = get_text(schema.url);
    let updated_at = get_i64(schema.updated_at);
    let is_internal_raw = get_i64(schema.is_internal);

    let updated_at_str = if updated_at > 0 {
        chrono::DateTime::from_timestamp(updated_at, 0).map(|dt| dt.to_rfc3339())
    } else {
        None
    };

    // Only surface the flag on comment hits; other entity types
    // get None so the frontend doesn't paint a misleading badge
    // on a ticket or device row.
    let is_internal = if entity_type == "comment" {
        Some(is_internal_raw == 1)
    } else {
        None
    };

    SearchResult {
        id,
        entity_type,
        entity_id,
        title,
        preview,
        url,
        score,
        updated_at: updated_at_str,
        is_internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::search::indexer::add_document_to_index;
    use crate::services::search::types::IndexDocument;
    use tantivy::Index;

    /// Build an in-RAM index, write the given docs, return a reader plus
    /// the schema so a test can drive `execute_search` directly without a
    /// database or disk.
    fn index_docs(docs: &[IndexDocument]) -> (Index, SearchSchema, IndexReader) {
        let schema = SearchSchema::new();
        let index = Index::create_in_ram(schema.schema.clone());
        {
            let mut writer = index.writer(15_000_000).expect("writer");
            for doc in docs {
                add_document_to_index(&writer, &schema, doc).expect("add doc");
            }
            writer.commit().expect("commit");
        }
        let reader = index.reader().expect("reader");
        (index, schema, reader)
    }

    fn ids(resp: &SearchResponse) -> Vec<String> {
        let mut v: Vec<String> = resp.results.iter().map(|r| r.id.clone()).collect();
        v.sort();
        v
    }

    fn ticket(id: i64, workspace_id: i64) -> IndexDocument {
        IndexDocument::new(
            EntityType::Ticket,
            id,
            "printer jam",
            "the printer is jammed",
        )
        .workspace_id(workspace_id)
    }

    #[test]
    fn search_is_gated_to_caller_workspace() {
        let docs = vec![ticket(1, 1), ticket(2, 2)];
        let (_index, schema, reader) = index_docs(&docs);

        let ws1 = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            1,
            SortOrder::Relevance,
        )
        .expect("ws1");
        assert_eq!(ids(&ws1), vec!["ticket-1"], "ws1 sees only its own ticket");

        let ws2 = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            2,
            SortOrder::Relevance,
        )
        .expect("ws2");
        assert_eq!(ids(&ws2), vec!["ticket-2"], "ws2 sees only its own ticket");
    }

    #[test]
    fn doc_with_no_workspace_is_unreachable() {
        // A doc indexed with an empty workspace set (fail-closed) must not
        // match any workspace query.
        let orphan = IndexDocument::new(EntityType::Ticket, 9, "printer jam", "jammed");
        assert!(orphan.workspace_ids.is_empty());
        let (_index, schema, reader) = index_docs(&[orphan]);

        for ws in [1i64, 2, 3] {
            let resp = execute_search(
                &reader,
                &schema,
                "printer",
                10,
                None,
                true,
                ws,
                SortOrder::Relevance,
            )
            .expect("q");
            assert!(
                resp.results.is_empty(),
                "orphan doc must be unreachable from workspace {ws}"
            );
        }
    }

    #[test]
    fn multi_valued_user_doc_matches_each_membership_only() {
        // A user in workspaces [1, 3] is reachable from 1 and 3, never 2.
        let user = IndexDocument::with_uuid(EntityType::User, "abc-def", "Sebastian")
            .workspace_ids(vec![1, 3]);
        let (_index, schema, reader) = index_docs(&[user]);

        let q = |ws: i64| {
            execute_search(
                &reader,
                &schema,
                "sebastian",
                10,
                None,
                true,
                ws,
                SortOrder::Relevance,
            )
            .expect("q")
            .results
            .len()
        };
        assert_eq!(q(1), 1, "reachable from workspace 1");
        assert_eq!(q(3), 1, "reachable from workspace 3");
        assert_eq!(q(2), 0, "not reachable from workspace 2 (no membership)");
    }

    #[test]
    fn workspace_gate_does_not_match_text_irrelevant_docs() {
        // Regression guard: adding the workspace Must clause must not turn
        // the text Should clauses into a no-op that returns every doc in
        // the workspace. Two docs in the same workspace, only one matches
        // the query term.
        let docs = vec![
            ticket(1, 1), // "printer jam"
            IndexDocument::new(
                EntityType::Ticket,
                2,
                "stapler broken",
                "the stapler is broken",
            )
            .workspace_id(1),
        ];
        let (_index, schema, reader) = index_docs(&docs);

        let resp = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            1,
            SortOrder::Relevance,
        )
        .expect("q");
        assert_eq!(
            ids(&resp),
            vec!["ticket-1"],
            "only the text-matching doc should return, not every doc in the workspace"
        );
    }

    #[test]
    fn workspace_gate_composes_with_entity_type_and_internal_filters() {
        // Same workspace, two entity types; the type filter still narrows
        // within the workspace, and the internal-note filter still applies.
        let docs = vec![
            ticket(1, 1),
            IndexDocument::new(
                EntityType::Comment,
                5,
                "printer note",
                "internal printer note",
            )
            .is_internal(true)
            .workspace_id(1),
        ];
        let (_index, schema, reader) = index_docs(&docs);

        // Staff (include_internal=true), restricted to tickets only.
        let only_tickets = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            Some(&[EntityType::Ticket]),
            true,
            1,
            SortOrder::Relevance,
        )
        .expect("tickets");
        assert_eq!(ids(&only_tickets), vec!["ticket-1"]);

        // Non-staff must not see the internal comment even within their ws.
        let non_staff = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            false,
            1,
            SortOrder::Relevance,
        )
        .expect("non-staff");
        assert_eq!(
            ids(&non_staff),
            vec!["ticket-1"],
            "internal comment filtered out for non-staff"
        );
    }

    #[test]
    fn updated_sort_orders_newest_first() {
        // Three equally-matching tickets in one workspace with distinct
        // updated_at values; the Updated sort returns them newest-first,
        // independent of BM25 score. (ids() sorts alphabetically, so compare
        // the result order directly here.)
        let docs = vec![
            ticket(1, 1).updated_at(1_000),
            ticket(2, 1).updated_at(3_000),
            ticket(3, 1).updated_at(2_000),
        ];
        let (_index, schema, reader) = index_docs(&docs);

        let resp = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            1,
            SortOrder::Updated,
        )
        .expect("q");
        let order: Vec<String> = resp.results.iter().map(|r| r.id.clone()).collect();
        assert_eq!(
            order,
            vec!["ticket-2", "ticket-3", "ticket-1"],
            "newest updated_at first, regardless of relevance"
        );
    }

    #[test]
    fn relevance_and_updated_sorts_can_disagree() {
        // A recently-updated weak match vs an older strong match: relevance
        // puts the strong match first, Updated puts the recent one first.
        // Guards against the branches accidentally collapsing to one axis.
        let strong_old = IndexDocument::new(
            EntityType::Ticket,
            1,
            "printer printer printer",
            "printer jam printer",
        )
        .workspace_id(1)
        .updated_at(1_000);
        let weak_new = IndexDocument::new(EntityType::Ticket, 2, "the office printer", "misc note")
            .workspace_id(1)
            .updated_at(9_000);
        let (_index, schema, reader) = index_docs(&[strong_old, weak_new]);

        let by_rel = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            1,
            SortOrder::Relevance,
        )
        .expect("rel");
        assert_eq!(
            by_rel.results.first().map(|r| r.id.as_str()),
            Some("ticket-1"),
            "strongest text match first by relevance"
        );

        let by_upd = execute_search(
            &reader,
            &schema,
            "printer",
            10,
            None,
            true,
            1,
            SortOrder::Updated,
        )
        .expect("upd");
        assert_eq!(
            by_upd.results.first().map(|r| r.id.as_str()),
            Some("ticket-2"),
            "most recently updated first by Updated sort"
        );
    }
}
