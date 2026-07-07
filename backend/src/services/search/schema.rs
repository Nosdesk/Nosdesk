//! Tantivy index schema definition

use tantivy::schema::{
    Field, IndexRecordOption, NumericOptions, Schema, SchemaBuilder, TextFieldIndexing,
    TextOptions, STORED, STRING,
};
use tantivy::Index;

/// Field names in the search index
pub mod fields {
    pub const ID: &str = "id";
    pub const ENTITY_TYPE: &str = "entity_type";
    pub const ENTITY_ID: &str = "entity_id";
    pub const TITLE: &str = "title";
    pub const CONTENT: &str = "content";
    pub const METADATA: &str = "metadata";
    pub const URL: &str = "url";
    pub const PREVIEW: &str = "preview";
    pub const UPDATED_AT: &str = "updated_at";
    /// 0 or 1. Drives both the search-result badge (so techs can
    /// tell at a glance that a hit is an internal note) and the
    /// non-staff visibility filter (User-role callers never see
    /// is_internal=1 documents — preventing accidental disclosure
    /// of working notes through full-text search).
    pub const IS_INTERNAL: &str = "is_internal";
    /// Workspace tenancy dimension. Multi-valued: an entity owned by one
    /// workspace carries a single value; a user (global identity, member
    /// of N workspaces) carries one value per membership. Every query
    /// requires a matching value, so a doc is reachable only from a
    /// workspace it belongs to.
    pub const WORKSPACE_ID: &str = "workspace_id";
}

/// Container for all schema fields
#[derive(Clone)]
pub struct SearchSchema {
    pub schema: Schema,
    pub id: Field,
    pub entity_type: Field,
    pub entity_id: Field,
    pub title: Field,
    pub content: Field,
    pub metadata: Field,
    pub url: Field,
    pub preview: Field,
    pub updated_at: Field,
    pub is_internal: Field,
    pub workspace_id: Field,
}

impl SearchSchema {
    /// Create a new search schema with all fields configured.
    ///
    /// All searchable text fields use Tantivy's `default` tokenizer
    /// (whitespace split, lowercase, no stemming). Partial-prefix
    /// matching ("seb" → "Sebastian", "https" → "HTTPS") is handled
    /// at *query* time via `PrefixQuery` in `searcher.rs` rather than
    /// at index time, which keeps the index small and lets a single
    /// indexed term satisfy both exact and prefix queries.
    pub fn new() -> Self {
        let mut builder = SchemaBuilder::new();

        // STRING fields - stored but not tokenized (exact match only)
        let id = builder.add_text_field(fields::ID, STRING | STORED);
        let entity_type = builder.add_text_field(fields::ENTITY_TYPE, STRING | STORED);
        let url = builder.add_text_field(fields::URL, STRING | STORED);

        // Stored-only fields (not searchable)
        let preview = builder.add_text_field(fields::PREVIEW, STORED);

        // Numeric fields
        let numeric_options = NumericOptions::default().set_stored();
        let entity_id = builder.add_i64_field(fields::ENTITY_ID, numeric_options);

        // updated_at is FAST as well as stored: STORED lets us read the
        // timestamp back onto each result, FAST lets the collector sort
        // by it (`order_by_fast_field`) for the "Newest" sort option.
        // Making it fast is a schema change — see is_compatible_with_index,
        // which treats a non-fast updated_at as a stale index and forces a
        // rebuild on deploy.
        let updated_at_options = NumericOptions::default().set_stored().set_fast();
        let updated_at = builder.add_i64_field(fields::UPDATED_AT, updated_at_options);

        // Indexed so the searcher can filter is_internal=1 documents
        // out for non-staff callers via a Must-Not clause.
        let is_internal_options = NumericOptions::default().set_stored().set_indexed();
        let is_internal = builder.add_i64_field(fields::IS_INTERNAL, is_internal_options);

        // Indexed so every query can require a matching workspace term.
        // Multi-valued in practice (users carry one value per membership);
        // a field is multi-valued simply by adding it more than once.
        let workspace_id_options = NumericOptions::default().set_stored().set_indexed();
        let workspace_id = builder.add_i64_field(fields::WORKSPACE_ID, workspace_id_options);

        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer("default")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);

        let title_options = TextOptions::default()
            .set_indexing_options(text_indexing.clone())
            .set_stored();
        let title = builder.add_text_field(fields::TITLE, title_options);

        let content_options = TextOptions::default().set_indexing_options(text_indexing.clone());
        let content = builder.add_text_field(fields::CONTENT, content_options);

        let metadata_options = TextOptions::default().set_indexing_options(text_indexing);
        let metadata = builder.add_text_field(fields::METADATA, metadata_options);

        let schema = builder.build();

        Self {
            schema,
            id,
            entity_type,
            entity_id,
            title,
            content,
            metadata,
            url,
            preview,
            updated_at,
            is_internal,
            workspace_id,
        }
    }

    /// All field names in schema order, for validation and lookup
    const FIELD_NAMES: &'static [&'static str] = &[
        fields::ID,
        fields::ENTITY_TYPE,
        fields::ENTITY_ID,
        fields::TITLE,
        fields::CONTENT,
        fields::METADATA,
        fields::URL,
        fields::PREVIEW,
        fields::UPDATED_AT,
        fields::IS_INTERNAL,
        fields::WORKSPACE_ID,
    ];

    /// Create a SearchSchema from an existing index by looking up field handles
    pub fn from_index(index: &Index) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let schema = index.schema();

        let get = |name: &str| -> Result<Field, Box<dyn std::error::Error + Send + Sync>> {
            schema
                .get_field(name)
                .map_err(|_| format!("Missing field: {name}").into())
        };

        Ok(Self {
            id: get(fields::ID)?,
            entity_type: get(fields::ENTITY_TYPE)?,
            entity_id: get(fields::ENTITY_ID)?,
            title: get(fields::TITLE)?,
            content: get(fields::CONTENT)?,
            metadata: get(fields::METADATA)?,
            url: get(fields::URL)?,
            preview: get(fields::PREVIEW)?,
            updated_at: get(fields::UPDATED_AT)?,
            is_internal: get(fields::IS_INTERNAL)?,
            workspace_id: get(fields::WORKSPACE_ID)?,
            schema,
        })
    }

    /// Check if an index has the expected schema fields *and* the
    /// expected tokenizer wiring. An index built before we settled on
    /// the default tokenizer (briefly tried prefix_ngram which
    /// incorrectly tokenized the whole title as one input) gets
    /// rebuilt automatically on next startup.
    pub fn is_compatible_with_index(index: &Index) -> bool {
        use tantivy::schema::FieldType;

        let schema = index.schema();
        let all_fields_present = Self::FIELD_NAMES
            .iter()
            .all(|name| schema.get_field(name).is_ok());
        if !all_fields_present {
            return false;
        }

        let uses_tokenizer = |name: &str, expected: &str| -> bool {
            let Ok(field) = schema.get_field(name) else {
                return false;
            };
            let entry = schema.get_field_entry(field);
            match entry.field_type() {
                FieldType::Str(opts) => opts
                    .get_indexing_options()
                    .map(|i| i.tokenizer() == expected)
                    .unwrap_or(false),
                _ => false,
            }
        };

        // updated_at must be a FAST field, or the "Newest" sort's
        // `order_by_fast_field` fails at query time. An index built before
        // updated_at became fast has all fields present and the right
        // tokenizers, so this is the marker that distinguishes it and
        // triggers an automatic rebuild on the next startup after deploy.
        let updated_at_is_fast = schema
            .get_field(fields::UPDATED_AT)
            .map(|f| schema.get_field_entry(f).is_fast())
            .unwrap_or(false);

        updated_at_is_fast
            && uses_tokenizer(fields::TITLE, "default")
            && uses_tokenizer(fields::METADATA, "default")
            && uses_tokenizer(fields::CONTENT, "default")
    }
}

impl Default for SearchSchema {
    fn default() -> Self {
        Self::new()
    }
}
