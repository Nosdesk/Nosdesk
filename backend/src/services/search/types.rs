//! Search service types and DTOs

use serde::{Deserialize, Serialize};

/// Entity types that can be indexed and searched
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    Ticket,
    Comment,
    Documentation,
    Attachment,
    Device,
    User,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Ticket => "ticket",
            EntityType::Comment => "comment",
            EntityType::Documentation => "documentation",
            EntityType::Attachment => "attachment",
            EntityType::Device => "device",
            EntityType::User => "user",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ticket" => Some(EntityType::Ticket),
            "comment" => Some(EntityType::Comment),
            "documentation" => Some(EntityType::Documentation),
            "attachment" => Some(EntityType::Attachment),
            "device" => Some(EntityType::Device),
            "user" => Some(EntityType::User),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A document to be indexed in the search engine
#[derive(Debug, Clone)]
pub struct IndexDocument {
    /// Unique identifier (e.g., "ticket-123", "user-abc-def")
    pub id: String,
    /// Type of entity
    pub entity_type: EntityType,
    /// Numeric ID for tickets/comments/devices, or 0 for UUID-based entities
    pub entity_id: i64,
    /// Title or name (primary search target, high boost)
    pub title: String,
    /// Body content (comments, documentation text)
    pub content: String,
    /// Additional metadata (serial number, hostname, email, etc.)
    pub metadata: String,
    /// Direct URL for navigation
    pub url: String,
    /// Preview snippet for display
    pub preview: String,
    /// Last updated timestamp (Unix timestamp)
    pub updated_at: i64,
}

impl IndexDocument {
    pub fn new(
        entity_type: EntityType,
        entity_id: i64,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let entity_type_str = entity_type.as_str();
        Self {
            id: format!("{}-{}", entity_type_str, entity_id),
            entity_type,
            entity_id,
            title: title.into(),
            content: content.into(),
            metadata: String::new(),
            url: String::new(),
            preview: String::new(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn with_uuid(entity_type: EntityType, uuid: &str, title: impl Into<String>) -> Self {
        Self {
            id: format!("{}-{}", entity_type.as_str(), uuid),
            entity_type,
            entity_id: 0,
            title: title.into(),
            content: String::new(),
            metadata: String::new(),
            url: String::new(),
            preview: String::new(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = metadata.into();
        self
    }


    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    pub fn preview(mut self, preview: impl Into<String>) -> Self {
        self.preview = preview.into();
        self
    }

    pub fn updated_at(mut self, timestamp: i64) -> Self {
        self.updated_at = timestamp;
        self
    }
}

/// A single search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Unique identifier
    pub id: String,
    /// Type of entity
    pub entity_type: String,
    /// Numeric entity ID (for tickets, comments, devices, documentation)
    pub entity_id: i64,
    /// Title or name
    pub title: String,
    /// Preview snippet
    pub preview: String,
    /// Direct URL for navigation
    pub url: String,
    /// Search relevance score
    pub score: f32,
    /// Last updated timestamp (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Search query parameters
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    /// Search query string
    pub q: String,
    /// Maximum number of results to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Entity types to search (comma-separated)
    #[serde(default)]
    pub types: Option<String>,
}

fn default_limit() -> usize {
    20
}

impl SearchQuery {
    pub fn entity_types(&self) -> Option<Vec<EntityType>> {
        self.types.as_ref().map(|types_str| {
            types_str
                .split(',')
                .filter_map(|s| EntityType::from_str(s.trim()))
                .collect()
        })
    }
}

/// Search response
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    /// Search results
    pub results: Vec<SearchResult>,
    /// Total number of matching documents
    pub total: usize,
    /// Original query
    pub query: String,
    /// Search duration in milliseconds
    pub took_ms: u64,
}

/// Index statistics
#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    /// Total number of indexed documents
    pub total_documents: u64,
    /// Documents by entity type
    pub by_type: std::collections::HashMap<String, u64>,
    /// Index size on disk in bytes
    pub index_size_bytes: u64,
    /// Whether the index is currently being rebuilt
    pub is_rebuilding: bool,
}
