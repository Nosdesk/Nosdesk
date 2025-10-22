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

-- Users table with authentication and profile fields
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    role user_role NOT NULL DEFAULT 'user',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    password_hash BYTEA NOT NULL DEFAULT '\x'::bytea,
    password_changed_at TIMESTAMP WITH TIME ZONE, -- Track when password was last changed for session invalidation
    pronouns VARCHAR(100),
    avatar_url VARCHAR(500),
    banner_url VARCHAR(500),
    avatar_thumb VARCHAR(500),
    microsoft_uuid UUID,
    mfa_secret VARCHAR(255), -- Base32 encoded TOTP secret
    mfa_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    mfa_backup_codes JSONB, -- Array of backup codes for MFA recovery
    passkey_credentials JSONB -- WebAuthn passkey credentials
);

-- User authentication identities for external auth
CREATE TABLE user_auth_identities (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider_type VARCHAR(50) NOT NULL, -- 'microsoft', 'google', etc.
    external_id VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    password_hash VARCHAR(255),
    UNIQUE(provider_type, external_id)
);

-- User emails for multiple email addresses per user
CREATE TABLE user_emails (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(email)
);

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
    notes TEXT,
    user_id INTEGER REFERENCES users(id),
    primary_user_uuid UUID,
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

-- Projects table
CREATE TABLE projects (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    status project_status NOT NULL DEFAULT 'active',
    start_date DATE,
    end_date DATE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Tickets table
CREATE TABLE tickets (
    id SERIAL PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    description TEXT,
    status ticket_status NOT NULL DEFAULT 'open',
    priority ticket_priority NOT NULL DEFAULT 'medium',
    requester_uuid UUID REFERENCES users(uuid),
    assignee_uuid UUID REFERENCES users(uuid),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMP WITH TIME ZONE
);

-- Ticket-device junction table for many-to-many relationship
CREATE TABLE ticket_devices (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (ticket_id, device_id)
);

-- Comments table
CREATE TABLE comments (
    id SERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Attachments table
CREATE TABLE attachments (
    id SERIAL PRIMARY KEY,
    url VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    comment_id INTEGER REFERENCES comments(id) ON DELETE CASCADE
);

-- Article contents for knowledge base
CREATE TABLE article_contents (
    id SERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    ticket_id INTEGER REFERENCES tickets(id)
);

-- Documentation pages - Notion-like with Yrs collaborative editing
CREATE TABLE documentation_pages (
    id SERIAL PRIMARY KEY,
    uuid UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    title VARCHAR(255) NOT NULL,
    slug VARCHAR(255) UNIQUE,
    icon VARCHAR(50), -- emoji or icon identifier
    cover_image VARCHAR(500), -- cover image URL
    status documentation_status NOT NULL DEFAULT 'draft',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID NOT NULL REFERENCES users(uuid),
    last_edited_by UUID NOT NULL REFERENCES users(uuid),
    parent_id INTEGER REFERENCES documentation_pages(id),
    ticket_id INTEGER REFERENCES tickets(id),
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
    estimated_reading_time INTEGER DEFAULT 0, -- in minutes
    word_count INTEGER DEFAULT 0,
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
    created_by UUID NOT NULL REFERENCES users(uuid),
    change_summary TEXT, -- Optional summary of changes
    word_count INTEGER DEFAULT 0,
    
    UNIQUE(page_id, revision_number)
);

-- Linked tickets for relationships between tickets
CREATE TABLE linked_tickets (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    linked_ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (ticket_id, linked_ticket_id)
);

-- Project-ticket junction table
CREATE TABLE project_tickets (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
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
    tenant_id VARCHAR(255)
);

-- Active user sessions for session management and revocation
CREATE TABLE active_sessions (
    id SERIAL PRIMARY KEY,
    session_token VARCHAR(64) NOT NULL UNIQUE, -- JWT token hash for lookup
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    device_name VARCHAR(255), -- e.g., "MacBook Pro - Chrome"
    ip_address TEXT, -- Store IP addresses as text for compatibility
    user_agent TEXT,
    location VARCHAR(255), -- e.g., "San Francisco, CA"
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    last_active TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    is_current BOOLEAN NOT NULL DEFAULT FALSE -- Mark the current session
);

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
    ip_address TEXT, -- Store IP addresses as text for compatibility
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
    ip_address TEXT, -- Store IP addresses as text for compatibility
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

-- Create indexes for better performance
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_uuid ON users(uuid);

-- Documentation system indexes
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

-- Existing indexes
CREATE INDEX idx_linked_tickets_ticket_id ON linked_tickets(ticket_id);
CREATE INDEX idx_linked_tickets_linked_ticket_id ON linked_tickets(linked_ticket_id);
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

-- Setup updated_at triggers
SELECT diesel_manage_updated_at('users');
SELECT diesel_manage_updated_at('user_auth_identities');
SELECT diesel_manage_updated_at('user_emails');
SELECT diesel_manage_updated_at('devices');
SELECT diesel_manage_updated_at('projects');
SELECT diesel_manage_updated_at('tickets');
SELECT diesel_manage_updated_at('comments');
SELECT diesel_manage_updated_at('documentation_pages');
SELECT diesel_manage_updated_at('documentation_revisions');

-- Set the sequence to continue from ID 1
SELECT setval('users_id_seq', 1, true);
