-- Groups table: User groups for organizing users
CREATE TABLE groups (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    color VARCHAR(7), -- Hex color code like #FF5733
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

CREATE INDEX idx_groups_uuid ON groups(uuid);
CREATE INDEX idx_groups_name ON groups(name);

-- User groups junction table: Many-to-many relationship between users and groups
CREATE TABLE user_groups (
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (user_uuid, group_id)
);

CREATE INDEX idx_user_groups_user ON user_groups(user_uuid);
CREATE INDEX idx_user_groups_group ON user_groups(group_id);

-- Ticket categories table: Categories for organizing tickets
CREATE TABLE ticket_categories (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    color VARCHAR(7), -- Hex color code
    icon VARCHAR(50), -- Icon identifier (e.g., 'folder', 'bug', 'feature')
    display_order INT NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

CREATE INDEX idx_ticket_categories_uuid ON ticket_categories(uuid);
CREATE INDEX idx_ticket_categories_active ON ticket_categories(is_active);
CREATE INDEX idx_ticket_categories_order ON ticket_categories(display_order);

-- Category group visibility junction table: Which groups can see which categories
-- If a category has NO entries here -> visible to ALL users (public)
-- If a category has entries here -> only visible to users in those groups
CREATE TABLE category_group_visibility (
    category_id INT NOT NULL REFERENCES ticket_categories(id) ON DELETE CASCADE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (category_id, group_id)
);

CREATE INDEX idx_category_visibility_category ON category_group_visibility(category_id);
CREATE INDEX idx_category_visibility_group ON category_group_visibility(group_id);

-- Add category_id column to tickets table
ALTER TABLE tickets ADD COLUMN category_id INT REFERENCES ticket_categories(id) ON DELETE SET NULL;

CREATE INDEX idx_tickets_category ON tickets(category_id);
