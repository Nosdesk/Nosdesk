CREATE TABLE group_includes (
    parent_group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    child_group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (parent_group_id, child_group_id),
    CHECK (parent_group_id != child_group_id)
);
CREATE INDEX idx_group_includes_parent ON group_includes(parent_group_id);
CREATE INDEX idx_group_includes_child ON group_includes(child_group_id);
