-- Notification types reference table
CREATE TABLE notification_types (
    id SERIAL PRIMARY KEY,
    code VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    description TEXT,
    category VARCHAR(50) NOT NULL,
    default_channels JSONB NOT NULL DEFAULT '["in_app"]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert initial notification types
INSERT INTO notification_types (code, name, description, category, default_channels) VALUES
    ('ticket_assigned', 'Assigned to Ticket', 'When you are assigned to a ticket', 'ticket', '["in_app", "email"]'),
    ('ticket_status_changed', 'Ticket Status Changed', 'When a ticket you are involved with changes status', 'ticket', '["in_app"]'),
    ('comment_added', 'New Comment', 'When someone comments on a ticket you are involved with', 'comment', '["in_app"]'),
    ('mentioned', 'Mentioned in Comment', 'When someone mentions you with @username', 'mention', '["in_app", "email"]'),
    ('ticket_created_requester', 'Ticket Created', 'When a ticket is created where you are the requester', 'ticket', '["in_app"]');

-- User notification preferences
CREATE TABLE notification_preferences (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    notification_type_id INT NOT NULL REFERENCES notification_types(id) ON DELETE CASCADE,
    channel VARCHAR(20) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_uuid, notification_type_id, channel)
);

-- Persistent notification storage
CREATE TABLE notifications (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    notification_type_id INT NOT NULL REFERENCES notification_types(id) ON DELETE CASCADE,
    entity_type VARCHAR(50) NOT NULL,
    entity_id INT NOT NULL,
    title VARCHAR(255) NOT NULL,
    body TEXT,
    metadata JSONB,
    channels_delivered JSONB NOT NULL DEFAULT '[]',
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    read_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Rate limiting for email notifications
CREATE TABLE notification_rate_limits (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    notification_type_id INT NOT NULL REFERENCES notification_types(id) ON DELETE CASCADE,
    entity_type VARCHAR(50) NOT NULL,
    entity_id INT NOT NULL,
    last_notified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_uuid, notification_type_id, entity_type, entity_id)
);

-- Indexes for performance
CREATE INDEX idx_notification_preferences_user ON notification_preferences(user_uuid);
CREATE INDEX idx_notification_preferences_lookup ON notification_preferences(user_uuid, notification_type_id, channel);

CREATE INDEX idx_notifications_user_unread ON notifications(user_uuid, is_read) WHERE is_read = FALSE;
CREATE INDEX idx_notifications_user_created ON notifications(user_uuid, created_at DESC);
CREATE INDEX idx_notifications_entity ON notifications(entity_type, entity_id);

CREATE INDEX idx_notification_rate_limits_lookup ON notification_rate_limits(user_uuid, notification_type_id, entity_type, entity_id);
