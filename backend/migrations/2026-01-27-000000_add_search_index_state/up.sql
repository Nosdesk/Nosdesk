-- Create search_index_state table to track indexing status for admin diagnostics
-- This enables monitoring of search index health and incremental catch-up indexing

CREATE TABLE search_index_state (
    id SERIAL PRIMARY KEY,
    entity_type VARCHAR(50) NOT NULL UNIQUE,
    last_indexed_at TIMESTAMPTZ,
    index_version INTEGER NOT NULL DEFAULT 1,
    document_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    last_error_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create index on entity_type for quick lookups
CREATE INDEX idx_search_index_state_entity_type ON search_index_state(entity_type);

-- Insert initial rows for each entity type
INSERT INTO search_index_state (entity_type) VALUES
    ('ticket'),
    ('comment'),
    ('documentation'),
    ('attachment'),
    ('device'),
    ('user');

-- Add a comment explaining the table's purpose
COMMENT ON TABLE search_index_state IS 'Tracks the state of the Tantivy search index for each entity type';
COMMENT ON COLUMN search_index_state.entity_type IS 'Type of entity being indexed (ticket, comment, documentation, etc.)';
COMMENT ON COLUMN search_index_state.last_indexed_at IS 'Timestamp of the last successful index operation';
COMMENT ON COLUMN search_index_state.index_version IS 'Version number for index schema changes';
COMMENT ON COLUMN search_index_state.document_count IS 'Number of documents of this type in the index';
COMMENT ON COLUMN search_index_state.last_error IS 'Description of the last indexing error, if any';
COMMENT ON COLUMN search_index_state.last_error_at IS 'Timestamp of the last indexing error';
