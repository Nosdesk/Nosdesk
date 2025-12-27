-- Assignment method enum
CREATE TYPE assignment_method AS ENUM (
    'direct_user',       -- Assign to a specific user
    'group_round_robin', -- Distribute evenly among group members
    'group_random',      -- Randomly pick a group member
    'group_queue'        -- Assign to group (users claim from queue)
);

-- Core assignment rules configuration
CREATE TABLE assignment_rules (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    priority INT NOT NULL DEFAULT 100,        -- Lower = higher priority
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    method assignment_method NOT NULL,
    target_user_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL,
    target_group_id INT REFERENCES groups(id) ON DELETE SET NULL,
    trigger_on_create BOOLEAN NOT NULL DEFAULT TRUE,
    trigger_on_category_change BOOLEAN NOT NULL DEFAULT TRUE,
    category_id INT REFERENCES ticket_categories(id) ON DELETE SET NULL,
    conditions JSONB DEFAULT '{}',            -- Extensible conditions for future enhancements
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Round-robin and assignment state tracking
CREATE TABLE assignment_rule_state (
    rule_id INT PRIMARY KEY REFERENCES assignment_rules(id) ON DELETE CASCADE,
    last_assigned_index INT NOT NULL DEFAULT 0,
    total_assignments INT NOT NULL DEFAULT 0,
    last_assigned_at TIMESTAMPTZ,
    last_assigned_user_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Assignment audit log
CREATE TABLE assignment_log (
    id SERIAL PRIMARY KEY,
    ticket_id INT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    rule_id INT REFERENCES assignment_rules(id) ON DELETE SET NULL,
    trigger_type VARCHAR(50) NOT NULL,        -- 'ticket_created', 'category_changed'
    previous_assignee_uuid UUID,
    new_assignee_uuid UUID,
    method assignment_method NOT NULL,
    context JSONB DEFAULT '{}',               -- Additional context (rule name at time, etc.)
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX idx_assignment_rules_priority ON assignment_rules(priority) WHERE is_active = TRUE;
CREATE INDEX idx_assignment_rules_category ON assignment_rules(category_id) WHERE is_active = TRUE;
CREATE INDEX idx_assignment_log_ticket ON assignment_log(ticket_id);
CREATE INDEX idx_assignment_log_assigned_at ON assignment_log(assigned_at);
