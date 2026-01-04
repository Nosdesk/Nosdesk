-- Device groups junction table: Many-to-many relationship between devices and groups
-- Follows the same pattern as user_groups table
CREATE TABLE device_groups (
    device_id INT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    -- Track if this membership was synced from external source or manually added
    -- "microsoft" for synced memberships, NULL for manual
    external_source VARCHAR(50),
    PRIMARY KEY (device_id, group_id)
);

CREATE INDEX idx_device_groups_device ON device_groups(device_id);
CREATE INDEX idx_device_groups_group ON device_groups(group_id);
CREATE INDEX idx_device_groups_external ON device_groups(external_source);
