-- Plugins table for installed plugins
CREATE TABLE plugins (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    name VARCHAR(100) UNIQUE NOT NULL,
    display_name VARCHAR(255) NOT NULL,
    version VARCHAR(50) NOT NULL,
    description TEXT,
    manifest JSONB NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    trust_level VARCHAR(50) NOT NULL DEFAULT 'community',
    installed_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Plugin settings (admin-configured)
CREATE TABLE plugin_settings (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key VARCHAR(100) NOT NULL,
    value JSONB,
    is_secret BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_id, key)
);

-- Plugin storage (plugin-managed data)
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

-- Plugin activity log (audit)
CREATE TABLE plugin_activity (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    action VARCHAR(100) NOT NULL,
    details JSONB,
    user_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX idx_plugins_enabled ON plugins(enabled) WHERE enabled = true;
CREATE INDEX idx_plugins_name ON plugins(name);
CREATE INDEX idx_plugin_settings_plugin_id ON plugin_settings(plugin_id);
CREATE INDEX idx_plugin_storage_plugin_id ON plugin_storage(plugin_id);
CREATE INDEX idx_plugin_storage_key ON plugin_storage(plugin_id, key);
CREATE INDEX idx_plugin_activity_plugin_id ON plugin_activity(plugin_id);
CREATE INDEX idx_plugin_activity_created_at ON plugin_activity(created_at DESC);

-- Auto-update updated_at timestamps
SELECT diesel_manage_updated_at('plugins');
SELECT diesel_manage_updated_at('plugin_settings');
SELECT diesel_manage_updated_at('plugin_storage');

COMMENT ON TABLE plugins IS 'Installed plugins with their manifests and configuration';
COMMENT ON TABLE plugin_settings IS 'Admin-configured settings for plugins (API keys, preferences)';
COMMENT ON TABLE plugin_storage IS 'Plugin-managed key-value storage for runtime data';
COMMENT ON TABLE plugin_activity IS 'Audit log of plugin actions';
COMMENT ON COLUMN plugins.trust_level IS 'Trust level: official, verified, or community';
COMMENT ON COLUMN plugin_settings.is_secret IS 'If true, value is encrypted and not exposed in API responses';
