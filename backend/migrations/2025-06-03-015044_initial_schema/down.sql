-- This file should undo anything in `up.sql`

-- Drop all tables in reverse order to handle foreign key dependencies

-- Drop security and session management tables first (they reference users)
DROP TABLE IF EXISTS user_ticket_views;
DROP TABLE IF EXISTS reset_tokens;
DROP TABLE IF EXISTS security_events;
DROP TABLE IF EXISTS refresh_tokens;
DROP TABLE IF EXISTS active_sessions;

DROP TABLE IF EXISTS sync_history;
DROP TABLE IF EXISTS project_tickets;
DROP TABLE IF EXISTS linked_tickets;

-- Drop documentation system tables (simplified)
DROP TABLE IF EXISTS documentation_revisions;
DROP TABLE IF EXISTS documentation_pages;

DROP TABLE IF EXISTS article_contents;
DROP TABLE IF EXISTS attachments;
DROP TABLE IF EXISTS comments;
DROP TABLE IF EXISTS ticket_devices;
DROP TABLE IF EXISTS tickets;
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS devices;
DROP TABLE IF EXISTS user_emails;
DROP TABLE IF EXISTS user_auth_identities;
DROP TABLE IF EXISTS users;

-- Drop custom types
DROP TYPE IF EXISTS user_role;
DROP TYPE IF EXISTS ticket_status;
DROP TYPE IF EXISTS ticket_priority;
DROP TYPE IF EXISTS project_status;
DROP TYPE IF EXISTS documentation_status;
