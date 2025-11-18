-- Your SQL goes here

-- Initial schema for Nosdesk helpdesk system

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create custom ENUM types
CREATE TYPE documentation_status AS ENUM (
    'draft',
    'published',
    'archived'
);

CREATE TYPE project_status AS ENUM (
    'active',
    'completed',
    'archived'
);

CREATE TYPE ticket_priority AS ENUM (
    'low',
    'medium',
    'high'
);

CREATE TYPE ticket_status AS ENUM (
    'open',
    'in-progress',
    'closed'
);

CREATE TYPE user_role AS ENUM (
    'admin',
    'technician',
    'user'
);

-- Users table with profile fields only (auth credentials moved to user_auth_identities)
-- UUID is now the primary key for better security and distributed system support
CREATE TABLE users (
    uuid UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    role user_role NOT NULL DEFAULT 'user',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    password_changed_at TIMESTAMP WITH TIME ZONE, -- Track when password was last changed for session invalidation
    pronouns VARCHAR(100),
    avatar_url VARCHAR(2048), -- Increased for longer URLs (S3, Azure blob, etc.)
    banner_url VARCHAR(2048),
    avatar_thumb VARCHAR(2048),
    theme VARCHAR(50) DEFAULT 'system', -- User color scheme preference (system/light/dark)
    microsoft_uuid UUID, -- Legacy field for Microsoft integration
    mfa_secret VARCHAR(255), -- Base32 encoded TOTP secret (encrypted)
    mfa_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    mfa_backup_codes JSONB, -- Array of backup codes for MFA recovery
    passkey_credentials JSONB -- WebAuthn passkey credentials
);

-- User authentication identities - now the ONLY place where auth credentials are stored
-- Supports both local (password) and OAuth (Microsoft, Google, etc.) authentication
CREATE TABLE user_auth_identities (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    provider_type VARCHAR(50) NOT NULL, -- 'local', 'microsoft', 'google', etc.
    external_id VARCHAR(255) NOT NULL, -- For local: user email, for OAuth: provider's user ID
    email VARCHAR(320), -- Max valid email length (320 chars)
    metadata JSONB, -- Provider-specific data (OAuth tokens, profile info, etc.)
    password_hash VARCHAR(255), -- Only set for local auth, NULL for OAuth
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL, -- Who created this auth method
    UNIQUE(provider_type, external_id)
);

-- User emails for multiple email addresses per user
-- This is the single source of truth for all user emails
CREATE TABLE user_emails (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    email VARCHAR(320) NOT NULL, -- Max valid email length
    email_type VARCHAR(50) NOT NULL DEFAULT 'personal', -- 'personal', 'work', 'other'
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    source VARCHAR(50), -- 'manual', 'microsoft', 'google', etc.
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    UNIQUE(email)
);

-- Ensure each user has only one primary email
CREATE UNIQUE INDEX user_emails_one_primary_per_user ON user_emails(user_uuid) WHERE is_primary = TRUE;

-- Email format validation constraint
ALTER TABLE user_emails ADD CONSTRAINT valid_email_format
    CHECK (email ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$');

-- Devices table
CREATE TABLE devices (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    hostname VARCHAR(255),
    device_type VARCHAR(100),
    serial_number VARCHAR(255),
    manufacturer VARCHAR(255),
    model VARCHAR(255),
    warranty_status VARCHAR(50),
    location VARCHAR(255),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    notes TEXT,
    primary_user_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL, -- Device's primary user
    azure_device_id VARCHAR(255),
    intune_device_id VARCHAR(255),
    entra_device_id VARCHAR(255),
    compliance_state VARCHAR(50),
    last_sync_time TIMESTAMP WITH TIME ZONE,
    operating_system VARCHAR(100),
    os_version VARCHAR(100),
    is_managed BOOLEAN DEFAULT FALSE,
    enrollment_date TIMESTAMP WITH TIME ZONE
);

-- Unique constraint on serial numbers (with nulls allowed)
CREATE UNIQUE INDEX idx_device_serial_unique ON devices(serial_number) WHERE serial_number IS NOT NULL;

-- Projects table
CREATE TABLE projects (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    status project_status NOT NULL DEFAULT 'active',
    start_date DATE,
    end_date DATE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    owner_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL -- Project owner
);

-- Ensure end_date is after start_date
ALTER TABLE projects ADD CONSTRAINT projects_dates_valid
    CHECK (end_date IS NULL OR start_date IS NULL OR end_date >= start_date);

-- Tickets table
CREATE TABLE tickets (
    id SERIAL PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    description TEXT,
    status ticket_status NOT NULL DEFAULT 'open',
    priority ticket_priority NOT NULL DEFAULT 'medium',
    requester_uuid UUID REFERENCES users(uuid) ON DELETE RESTRICT, -- Prevent deletion of users with tickets
    assignee_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL, -- Keep ticket if assignee deleted
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    closed_at TIMESTAMP WITH TIME ZONE,
    closed_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Ensure closed_at is after created_at
ALTER TABLE tickets ADD CONSTRAINT tickets_dates_valid
    CHECK (closed_at IS NULL OR closed_at >= created_at);

-- Ticket-device junction table for many-to-many relationship
CREATE TABLE ticket_devices (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (ticket_id, device_id)
);

-- Comments table
CREATE TABLE comments (
    id SERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE RESTRICT, -- Prevent deletion of users with comments
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    is_edited BOOLEAN NOT NULL DEFAULT FALSE,
    edit_count INTEGER NOT NULL DEFAULT 0
);

-- Attachments table with full metadata
CREATE TABLE attachments (
    id SERIAL PRIMARY KEY,
    url VARCHAR(2048) NOT NULL, -- Increased for cloud storage URLs
    name VARCHAR(255) NOT NULL,
    file_size BIGINT, -- File size in bytes
    mime_type VARCHAR(100), -- Content type
    checksum VARCHAR(64), -- SHA256 hash for integrity
    comment_id INTEGER REFERENCES comments(id) ON DELETE CASCADE,
    uploaded_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Article contents for knowledge base
CREATE TABLE article_contents (
    id SERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE CASCADE,
    current_revision_number INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Article content revisions for version history
-- Simplified schema: state_vector + full_update (removed redundant snapshot field)
CREATE TABLE article_content_revisions (
    id SERIAL PRIMARY KEY,
    article_content_id INTEGER NOT NULL REFERENCES article_contents(id) ON DELETE CASCADE,
    revision_number INTEGER NOT NULL,
    yjs_state_vector BYTEA NOT NULL,        -- State vector at revision time
    yjs_document_content BYTEA NOT NULL,    -- Full Yjs update (V1 encoded)
    contributed_by UUID[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(article_content_id, revision_number)
);

CREATE INDEX idx_article_content_revisions_article_id ON article_content_revisions(article_content_id);
CREATE INDEX idx_article_content_revisions_contributors ON article_content_revisions USING GIN(contributed_by);

-- Documentation pages - Notion-like with Yrs collaborative editing
CREATE TABLE documentation_pages (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    title VARCHAR(255) NOT NULL,
    slug VARCHAR(255) UNIQUE,
    icon VARCHAR(50), -- emoji or icon identifier
    cover_image VARCHAR(2048), -- cover image URL
    status documentation_status NOT NULL DEFAULT 'draft',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID NOT NULL REFERENCES users(uuid) ON DELETE RESTRICT,
    last_edited_by UUID NOT NULL REFERENCES users(uuid) ON DELETE RESTRICT,
    parent_id INTEGER REFERENCES documentation_pages(id) ON DELETE CASCADE,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
    display_order INTEGER DEFAULT 0,

    -- Permissions baked into the page
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    is_template BOOLEAN NOT NULL DEFAULT FALSE,
    archived_at TIMESTAMP WITH TIME ZONE,

    -- Yrs document state (current version)
    yjs_state_vector BYTEA, -- Current state vector for sync
    yjs_document BYTEA, -- Current Yjs document binary state
    yjs_client_id BIGINT, -- Last client ID that updated this document

    -- Metadata
    has_unsaved_changes BOOLEAN NOT NULL DEFAULT FALSE
);

-- Optional: Simple revision history (major versions only)
CREATE TABLE documentation_revisions (
    id SERIAL PRIMARY KEY,
    page_id INTEGER NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    revision_number INTEGER NOT NULL,
    title VARCHAR(255) NOT NULL,
    yjs_document_snapshot BYTEA NOT NULL, -- Full Yjs document at this revision
    yjs_state_vector BYTEA NOT NULL, -- State vector at this revision
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID NOT NULL REFERENCES users(uuid) ON DELETE RESTRICT,
    change_summary TEXT, -- Optional summary of changes

    UNIQUE(page_id, revision_number)
);

-- Linked tickets for relationships between tickets
CREATE TABLE linked_tickets (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    linked_ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    link_type VARCHAR(50) NOT NULL DEFAULT 'relates_to', -- 'blocks', 'blocked_by', 'relates_to', 'duplicates'
    description TEXT, -- Optional context for the relationship
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (ticket_id, linked_ticket_id),
    -- Prevent self-referencing tickets
    CONSTRAINT no_self_link CHECK (ticket_id != linked_ticket_id)
);

-- Project-ticket junction table
CREATE TABLE project_tickets (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (project_id, ticket_id)
);

-- Sync history for external integrations
CREATE TABLE sync_history (
    id SERIAL PRIMARY KEY,
    sync_type VARCHAR(100) NOT NULL,
    status VARCHAR(50) NOT NULL,
    started_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMP WITH TIME ZONE,
    error_message TEXT,
    records_processed INTEGER DEFAULT 0,
    records_created INTEGER DEFAULT 0,
    records_updated INTEGER DEFAULT 0,
    records_failed INTEGER DEFAULT 0,
    tenant_id VARCHAR(255),
    initiated_by UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Active user sessions for session management and revocation
CREATE TABLE active_sessions (
    id SERIAL PRIMARY KEY,
    session_token VARCHAR(64) NOT NULL UNIQUE, -- JWT token hash for lookup
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    device_name VARCHAR(255), -- e.g., "MacBook Pro - Chrome"
    ip_address INET, -- PostgreSQL native IP address type
    user_agent TEXT,
    location VARCHAR(255), -- e.g., "San Francisco, CA"
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    last_active TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    is_current BOOLEAN NOT NULL DEFAULT FALSE -- Mark the current session
);

-- Ensure expires_at is after created_at
ALTER TABLE active_sessions ADD CONSTRAINT session_times_valid
    CHECK (expires_at > created_at);

-- Refresh tokens for JWT token rotation
CREATE TABLE refresh_tokens (
    id SERIAL PRIMARY KEY,
    token_hash VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 hash of refresh token
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    revoked_at TIMESTAMP WITH TIME ZONE -- NULL if still valid
);

-- Security events for MFA and authentication monitoring
CREATE TABLE security_events (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL, -- 'mfa_failed', 'login_failed', 'mfa_enabled', etc.
    ip_address INET, -- PostgreSQL native IP address type
    user_agent TEXT,
    location VARCHAR(255), -- Geographic location derived from IP
    details JSONB, -- Additional event-specific data
    severity VARCHAR(20) NOT NULL DEFAULT 'info', -- 'info', 'warning', 'critical'
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),

    -- Optional foreign keys for context
    session_id INTEGER REFERENCES active_sessions(id) ON DELETE SET NULL
);

-- Generic reset tokens for password resets, MFA resets, and other temporary tokens
CREATE TABLE reset_tokens (
    token_hash VARCHAR(64) NOT NULL PRIMARY KEY, -- SHA-256 hash of the token
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    token_type VARCHAR(50) NOT NULL, -- 'password_reset', 'mfa_reset', etc.
    ip_address INET, -- PostgreSQL native IP address type
    user_agent TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL, -- Configurable per token type
    used_at TIMESTAMP WITH TIME ZONE,
    is_used BOOLEAN NOT NULL DEFAULT FALSE,

    -- Flexible metadata for token-type-specific data (MFA admin approval, etc.)
    metadata JSONB
);

-- User ticket views for tracking recently viewed tickets
CREATE TABLE user_ticket_views (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    first_viewed_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    last_viewed_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    view_count INTEGER NOT NULL DEFAULT 1,
    UNIQUE(user_uuid, ticket_id)
);

-- ============================================================================
-- INDEXES - Optimized for common query patterns
-- ============================================================================

-- Users indexes
CREATE INDEX idx_users_uuid ON users(uuid);
CREATE INDEX idx_users_role ON users(role);
CREATE INDEX idx_users_created_at ON users(created_at DESC);

-- User auth identities indexes
CREATE INDEX idx_user_auth_identities_user_uuid ON user_auth_identities(user_uuid);
CREATE INDEX idx_user_auth_identities_provider_type ON user_auth_identities(provider_type);
CREATE INDEX idx_user_auth_identities_external_id ON user_auth_identities(external_id);

-- User emails indexes
CREATE INDEX idx_user_emails_email ON user_emails(email);
CREATE INDEX idx_user_emails_user_uuid ON user_emails(user_uuid);
CREATE INDEX idx_user_emails_is_primary ON user_emails(user_uuid, is_primary);
CREATE INDEX idx_user_emails_verified ON user_emails(email) WHERE is_verified = true;

-- Devices indexes
CREATE INDEX idx_devices_primary_user ON devices(primary_user_uuid) WHERE primary_user_uuid IS NOT NULL;
CREATE INDEX idx_devices_serial_number ON devices(serial_number) WHERE serial_number IS NOT NULL;
CREATE INDEX idx_devices_created_at ON devices(created_at DESC);

-- Projects indexes
CREATE INDEX idx_projects_status ON projects(status);
CREATE INDEX idx_projects_owner ON projects(owner_uuid) WHERE owner_uuid IS NOT NULL;
CREATE INDEX idx_projects_created_at ON projects(created_at DESC);

-- Tickets indexes
CREATE INDEX idx_tickets_status ON tickets(status);
CREATE INDEX idx_tickets_priority ON tickets(priority);
CREATE INDEX idx_tickets_status_priority ON tickets(status, priority);
CREATE INDEX idx_tickets_requester ON tickets(requester_uuid) WHERE requester_uuid IS NOT NULL;
CREATE INDEX idx_tickets_assignee ON tickets(assignee_uuid) WHERE assignee_uuid IS NOT NULL;
CREATE INDEX idx_tickets_created_at ON tickets(created_at DESC);
CREATE INDEX idx_tickets_closed_at ON tickets(closed_at DESC) WHERE closed_at IS NOT NULL;

-- Comments indexes
CREATE INDEX idx_comments_ticket_id ON comments(ticket_id);
CREATE INDEX idx_comments_user_uuid ON comments(user_uuid);
CREATE INDEX idx_comments_ticket_created ON comments(ticket_id, created_at DESC);

-- Attachments indexes
CREATE INDEX idx_attachments_comment_id ON attachments(comment_id);
CREATE INDEX idx_attachments_uploaded_by ON attachments(uploaded_by);

-- Documentation pages indexes
CREATE INDEX idx_documentation_pages_parent_id ON documentation_pages(parent_id);
CREATE INDEX idx_documentation_pages_ticket_id ON documentation_pages(ticket_id);
CREATE INDEX idx_documentation_pages_display_order ON documentation_pages(display_order);
CREATE INDEX idx_documentation_pages_uuid ON documentation_pages(uuid);
CREATE INDEX idx_documentation_pages_slug ON documentation_pages(slug);
CREATE INDEX idx_documentation_pages_status ON documentation_pages(status);
CREATE INDEX idx_documentation_pages_created_by ON documentation_pages(created_by);

-- Revision history indexes
CREATE INDEX idx_documentation_revisions_page_id ON documentation_revisions(page_id);
CREATE INDEX idx_documentation_revisions_created_at ON documentation_revisions(created_at);
CREATE INDEX idx_documentation_revisions_revision_number ON documentation_revisions(page_id, revision_number);

-- Linked tickets indexes
CREATE INDEX idx_linked_tickets_ticket_id ON linked_tickets(ticket_id);
CREATE INDEX idx_linked_tickets_linked_ticket_id ON linked_tickets(linked_ticket_id);
CREATE INDEX idx_linked_tickets_link_type ON linked_tickets(link_type);

-- Project tickets indexes
CREATE INDEX idx_project_tickets_project_id ON project_tickets(project_id);
CREATE INDEX idx_project_tickets_ticket_id ON project_tickets(ticket_id);

-- Active sessions indexes
CREATE INDEX idx_active_sessions_user_uuid ON active_sessions(user_uuid);
CREATE INDEX idx_active_sessions_session_token ON active_sessions(session_token);
CREATE INDEX idx_active_sessions_expires_at ON active_sessions(expires_at);
CREATE INDEX idx_active_sessions_last_active ON active_sessions(last_active);
CREATE INDEX idx_active_sessions_ip_address ON active_sessions(ip_address);

-- Refresh tokens indexes
CREATE INDEX idx_refresh_tokens_user_uuid ON refresh_tokens(user_uuid);
CREATE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);

-- Security events indexes
CREATE INDEX idx_security_events_user_uuid ON security_events(user_uuid);
CREATE INDEX idx_security_events_event_type ON security_events(event_type);
CREATE INDEX idx_security_events_created_at ON security_events(created_at);
CREATE INDEX idx_security_events_severity ON security_events(severity);
CREATE INDEX idx_security_events_ip_address ON security_events(ip_address);
CREATE INDEX idx_security_events_session_id ON security_events(session_id);
CREATE INDEX idx_security_events_user_created ON security_events(user_uuid, created_at DESC);

-- Reset tokens indexes
CREATE INDEX idx_reset_tokens_user_uuid ON reset_tokens(user_uuid);
CREATE INDEX idx_reset_tokens_token_type ON reset_tokens(token_type);
CREATE INDEX idx_reset_tokens_expires_at ON reset_tokens(expires_at);
CREATE INDEX idx_reset_tokens_created_at ON reset_tokens(created_at);
CREATE INDEX idx_reset_tokens_is_used ON reset_tokens(is_used);
CREATE INDEX idx_reset_tokens_user_type ON reset_tokens(user_uuid, token_type);

-- User ticket views indexes
CREATE INDEX idx_user_ticket_views_user_uuid ON user_ticket_views(user_uuid);
CREATE INDEX idx_user_ticket_views_ticket_id ON user_ticket_views(ticket_id);
CREATE INDEX idx_user_ticket_views_last_viewed_at ON user_ticket_views(last_viewed_at);
CREATE INDEX idx_user_ticket_views_user_last_viewed ON user_ticket_views(user_uuid, last_viewed_at DESC);

-- ============================================================================
-- TRIGGERS - Auto-update timestamps
-- ============================================================================

SELECT diesel_manage_updated_at('users');
SELECT diesel_manage_updated_at('user_auth_identities');
SELECT diesel_manage_updated_at('user_emails');
SELECT diesel_manage_updated_at('devices');
SELECT diesel_manage_updated_at('projects');
SELECT diesel_manage_updated_at('tickets');
SELECT diesel_manage_updated_at('comments');
SELECT diesel_manage_updated_at('documentation_pages');
SELECT diesel_manage_updated_at('documentation_revisions');
