CREATE TABLE plugin_collection_schemas (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    collection_name VARCHAR(100) NOT NULL,
    schema JSONB NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_id, collection_name)
);

CREATE TABLE plugin_collection_rows (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    plugin_id INTEGER NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    schema_id INTEGER NOT NULL REFERENCES plugin_collection_schemas(id) ON DELETE CASCADE,
    data JSONB NOT NULL DEFAULT '{}',
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pcschemas_plugin ON plugin_collection_schemas(plugin_id);
CREATE INDEX idx_pcrows_schema ON plugin_collection_rows(schema_id);
CREATE INDEX idx_pcrows_plugin ON plugin_collection_rows(plugin_id);
CREATE INDEX idx_pcrows_data ON plugin_collection_rows USING GIN (data);

SELECT diesel_manage_updated_at('plugin_collection_schemas');
SELECT diesel_manage_updated_at('plugin_collection_rows');
