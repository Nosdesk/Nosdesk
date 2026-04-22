-- Database initialization script for Nosdesk
-- This script ensures the database and user are properly configured

-- Create database if it doesn't exist (PostgreSQL)
-- Note: In Docker, the database is already created via POSTGRES_DB environment variable

-- Ensure the user has the necessary permissions
GRANT ALL PRIVILEGES ON DATABASE helpdesk TO nosdesk;

-- Separate database for the Rust test suite. Tests wrap each run in a
-- rolled-back transaction for row isolation, but PostgreSQL sequences
-- are non-transactional — every `cargo test` would otherwise burn IDs
-- out of the dev database's ticket / user / comment sequences, making
-- ticket numbers jump into the thousands after a handful of runs.
CREATE DATABASE helpdesk_test;
GRANT ALL PRIVILEGES ON DATABASE helpdesk_test TO nosdesk;

-- Create any additional schema or initial data here if needed
-- The actual table schema will be created by Diesel migrations