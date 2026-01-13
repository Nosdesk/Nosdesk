-- Consolidate plugin_settings and plugin_storage into a single plugin_data table
-- This simplifies the schema while maintaining the distinction via data_type column

-- Create the consolidated table
CREATE TABLE plugin_data (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    data_type VARCHAR(20) NOT NULL CHECK (data_type IN ('setting', 'storage')),
    key VARCHAR(255) NOT NULL,
    value JSONB,
    is_secret BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_id, data_type, key)
);

-- Migrate settings data
INSERT INTO plugin_data (uuid, plugin_id, data_type, key, value, is_secret, created_at, updated_at)
SELECT uuid, plugin_id, 'setting', key, value, is_secret, created_at, updated_at
FROM plugin_settings;

-- Migrate storage data
INSERT INTO plugin_data (uuid, plugin_id, data_type, key, value, is_secret, created_at, updated_at)
SELECT uuid, plugin_id, 'storage', key, value, FALSE, created_at, updated_at
FROM plugin_storage;

-- Create indexes
CREATE INDEX idx_plugin_data_plugin_id ON plugin_data(plugin_id);
CREATE INDEX idx_plugin_data_type ON plugin_data(data_type);
CREATE INDEX idx_plugin_data_plugin_type ON plugin_data(plugin_id, data_type);

-- Add trigger for updated_at
SELECT diesel_manage_updated_at('plugin_data');

-- Drop old tables
DROP TABLE plugin_settings;
DROP TABLE plugin_storage;
