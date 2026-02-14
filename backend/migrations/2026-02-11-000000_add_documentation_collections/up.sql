-- Documentation Collections: multi-tag grouping with group-based visibility
-- Collections allow documents to belong to multiple organizational groups
-- Visibility follows the same pattern as category_group_visibility:
--   No entries = public (visible to all), entries = restricted to listed groups

CREATE TABLE documentation_collections (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    icon VARCHAR(50),
    color VARCHAR(7),
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_documentation_collections_slug ON documentation_collections(slug);
CREATE INDEX idx_documentation_collections_created_by ON documentation_collections(created_by);

-- Junction table: many-to-many between collections and pages
CREATE TABLE documentation_collection_pages (
    collection_id INT NOT NULL REFERENCES documentation_collections(id) ON DELETE CASCADE,
    page_id INT NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (collection_id, page_id)
);

CREATE INDEX idx_collection_pages_collection ON documentation_collection_pages(collection_id);
CREATE INDEX idx_collection_pages_page ON documentation_collection_pages(page_id);

-- Visibility: which groups can see which collections
-- Same pattern as category_group_visibility
CREATE TABLE documentation_collection_visibility (
    collection_id INT NOT NULL REFERENCES documentation_collections(id) ON DELETE CASCADE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (collection_id, group_id)
);

CREATE INDEX idx_collection_visibility_collection ON documentation_collection_visibility(collection_id);
CREATE INDEX idx_collection_visibility_group ON documentation_collection_visibility(group_id);

-- Seed system collections
INSERT INTO documentation_collections (uuid, name, slug, description, icon, is_system)
VALUES
    (gen_random_uuid(), 'Tickets', 'tickets', 'Documentation pages created from ticket notes', '🎫', TRUE),
    (gen_random_uuid(), 'Getting Started', 'getting-started', 'Introduction and onboarding documentation', '🚀', TRUE);

COMMENT ON TABLE documentation_collections IS 'Organizational groupings for documentation pages, with group-based visibility';
COMMENT ON TABLE documentation_collection_pages IS 'Many-to-many junction between collections and documentation pages';
COMMENT ON TABLE documentation_collection_visibility IS 'Group-based visibility for collections (empty = public, entries = restricted)';
