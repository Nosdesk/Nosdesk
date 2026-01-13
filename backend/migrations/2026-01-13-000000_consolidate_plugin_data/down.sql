-- Restore the original separate tables

-- Recreate plugin_settings
CREATE TABLE plugin_settings (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key VARCHAR(255) NOT NULL,
    value JSONB,
    is_secret BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_id, key)
);

-- Recreate plugin_storage
CREATE TABLE plugin_storage (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key VARCHAR(255) NOT NULL,
    value JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_id, key)
);

-- Migrate data back
INSERT INTO plugin_settings (uuid, plugin_id, key, value, is_secret, created_at, updated_at)
SELECT uuid, plugin_id, key, value, is_secret, created_at, updated_at
FROM plugin_data WHERE data_type = 'setting';

INSERT INTO plugin_storage (uuid, plugin_id, key, value, created_at, updated_at)
SELECT uuid, plugin_id, key, value, created_at, updated_at
FROM plugin_data WHERE data_type = 'storage';

-- Create indexes
CREATE INDEX idx_plugin_settings_plugin_id ON plugin_settings(plugin_id);
CREATE INDEX idx_plugin_storage_plugin_id ON plugin_storage(plugin_id);

-- Add triggers
SELECT diesel_manage_updated_at('plugin_settings');
SELECT diesel_manage_updated_at('plugin_storage');

-- Drop consolidated table
DROP TABLE plugin_data;
