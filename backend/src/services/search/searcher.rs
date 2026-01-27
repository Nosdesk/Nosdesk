//! Search query execution

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
use tantivy::schema::IndexRecordOption;
use tantivy::{IndexReader, TantivyDocument};
use tracing::{debug, warn};

use super::schema::SearchSchema;
use super::types::{EntityType, SearchResult, SearchResponse};

/// Execute a search query against the index
pub fn execute_search(
    reader: &IndexReader,
    schema: &SearchSchema,
    query_str: &str,
    limit: usize,
    entity_types: Option<&[EntityType]>,
) -> Result<SearchResponse, Box<dyn std::error::Error + Send + Sync>> {
    let start_time = std::time::Instant::now();

    let searcher = reader.searcher();

    // Build the query
    let query = build_search_query(schema, query_str, entity_types);

    // Execute the search
    let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

    let total = top_docs.len();

    // Convert results
    let results: Vec<SearchResult> = top_docs
        .into_iter()
        .filter_map(|(score, doc_address)| {
            match searcher.doc::<TantivyDocument>(doc_address) {
                Ok(doc) => Some(document_to_result(&doc, schema, score)),
                Err(e) => {
                    warn!(error = ?e, "Failed to retrieve document");
                    None
                }
            }
        })
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

    let mut subqueries: Vec<(Occur, Box<dyn Query>)> = vec![
        (Occur::Should, title_query),
        (Occur::Should, content_query),
        (Occur::Should, metadata_query),
    ];

    // Add entity type filter if specified
    if let Some(types) = entity_types {
        if !types.is_empty() {
            let type_queries: Vec<(Occur, Box<dyn Query>)> = types
                .iter()
                .map(|t| {
                    let term = tantivy::Term::from_field_text(schema.entity_type, t.as_str());
                    let q: Box<dyn Query> = Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                    (Occur::Should, q)
                })
                .collect();

            let type_filter = BooleanQuery::new(type_queries);
            subqueries.push((Occur::Must, Box::new(type_filter)));
        }
    }

    Box::new(BooleanQuery::new(subqueries))
}

/// Build a term query for a specific field
/// Splits the query string into words and creates a boolean OR query
fn build_field_query(field: tantivy::schema::Field, query_str: &str) -> BooleanQuery {
    let terms: Vec<(Occur, Box<dyn Query>)> = query_str
        .split_whitespace()
        .map(|term| {
            let tantivy_term = tantivy::Term::from_field_text(field, &term.to_lowercase());
            let q: Box<dyn Query> = Box::new(TermQuery::new(tantivy_term, IndexRecordOption::WithFreqsAndPositions));
            (Occur::Should, q)
        })
        .collect();

    BooleanQuery::new(terms)
}

/// Convert a Tantivy document to a SearchResult
fn document_to_result(doc: &TantivyDocument, schema: &SearchSchema, score: f32) -> SearchResult {
    use tantivy::schema::OwnedValue;

    let get_text = |field: tantivy::schema::Field| -> String {
        doc.get_first(field)
            .and_then(|v: &OwnedValue| match v {
                OwnedValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };

    let get_i64 = |field: tantivy::schema::Field| -> i64 {
        doc.get_first(field)
            .and_then(|v: &OwnedValue| match v {
                OwnedValue::I64(i) => Some(*i),
                OwnedValue::U64(u) => Some(*u as i64),
                _ => None,
            })
            .unwrap_or(0)
    };

    let id = get_text(schema.id);
    let entity_type = get_text(schema.entity_type);
    let entity_id = get_i64(schema.entity_id);
    let title = get_text(schema.title);
    let preview = get_text(schema.preview);
    let url = get_text(schema.url);
    let updated_at = get_i64(schema.updated_at);

    let updated_at_str = if updated_at > 0 {
        chrono::DateTime::from_timestamp(updated_at, 0)
            .map(|dt| dt.to_rfc3339())
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
    }
}
