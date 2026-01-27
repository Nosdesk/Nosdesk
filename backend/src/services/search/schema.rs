//! Tantivy index schema definition

use tantivy::schema::{
    Field, Schema, SchemaBuilder, STORED, STRING, TextFieldIndexing, TextOptions,
    IndexRecordOption, NumericOptions,
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
}

impl SearchSchema {
    /// Create a new search schema with all fields configured
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
        let entity_id = builder.add_i64_field(fields::ENTITY_ID, numeric_options.clone());
        let updated_at = builder.add_i64_field(fields::UPDATED_AT, numeric_options);

        // TEXT fields - tokenized for full-text search
        // Title field with higher weight (configured at query time via boost)
        let title_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let title = builder.add_text_field(fields::TITLE, title_options);

        // Content field - main body text
        let content_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            );
        let content = builder.add_text_field(fields::CONTENT, content_options);

        // Metadata field - serial numbers, hostnames, emails, etc.
        let metadata_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            );
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
        }
    }

    /// All field names in schema order, for validation and lookup
    const FIELD_NAMES: &'static [&'static str] = &[
        fields::ID, fields::ENTITY_TYPE, fields::ENTITY_ID,
        fields::TITLE, fields::CONTENT, fields::METADATA,
        fields::URL, fields::PREVIEW, fields::UPDATED_AT,
    ];

    /// Create a SearchSchema from an existing index by looking up field handles
    pub fn from_index(index: &Index) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let schema = index.schema();

        let get = |name: &str| -> Result<Field, Box<dyn std::error::Error + Send + Sync>> {
            schema.get_field(name).map_err(|_| format!("Missing field: {name}").into())
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
            schema,
        })
    }

    /// Check if an index has the expected schema fields
    pub fn is_compatible_with_index(index: &Index) -> bool {
        let schema = index.schema();
        Self::FIELD_NAMES.iter().all(|name| schema.get_field(name).is_ok())
    }
}

impl Default for SearchSchema {
    fn default() -> Self {
        Self::new()
    }
}
