-- Add external sync fields to groups table for Microsoft Graph integration
ALTER TABLE groups ADD COLUMN external_id VARCHAR(255);
ALTER TABLE groups ADD COLUMN external_source VARCHAR(50);
ALTER TABLE groups ADD COLUMN group_type VARCHAR(50);
ALTER TABLE groups ADD COLUMN mail_enabled BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE groups ADD COLUMN security_enabled BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE groups ADD COLUMN last_synced_at TIMESTAMPTZ;
ALTER TABLE groups ADD COLUMN sync_enabled BOOLEAN NOT NULL DEFAULT true;

-- Unique index on external_id (only for non-null values)
CREATE UNIQUE INDEX idx_groups_external_id ON groups(external_id) WHERE external_id IS NOT NULL;
CREATE INDEX idx_groups_external_source ON groups(external_source);
CREATE INDEX idx_groups_group_type ON groups(group_type);

-- Delta tokens table for storing sync state (delta links for incremental sync)
CREATE TABLE sync_delta_tokens (
    id SERIAL PRIMARY KEY,
    provider_type VARCHAR(50) NOT NULL,
    entity_type VARCHAR(50) NOT NULL,
    delta_link TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(provider_type, entity_type)
);

CREATE INDEX idx_sync_delta_tokens_provider ON sync_delta_tokens(provider_type);
