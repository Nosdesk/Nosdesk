-- Create API tokens table for programmatic API access
CREATE TABLE api_tokens (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,      -- SHA-256 hash of token
    token_prefix VARCHAR(8) NOT NULL,            -- First 8 chars for identification (nsk_xxxx)
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,                  -- User-friendly name
    scopes TEXT[] DEFAULT ARRAY['full'],         -- Permission scopes
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by UUID NOT NULL REFERENCES users(uuid),
    expires_at TIMESTAMP WITH TIME ZONE,         -- NULL = no expiration
    revoked_at TIMESTAMP WITH TIME ZONE,         -- Soft delete
    last_used_at TIMESTAMP WITH TIME ZONE,
    last_used_ip INET
);

-- Index for looking up tokens by hash (primary auth lookup)
CREATE INDEX idx_api_tokens_token_hash ON api_tokens(token_hash);

-- Index for listing tokens by user
CREATE INDEX idx_api_tokens_user_uuid ON api_tokens(user_uuid);

-- Index for listing active (non-revoked) tokens
CREATE INDEX idx_api_tokens_revoked_at ON api_tokens(revoked_at) WHERE revoked_at IS NULL;

COMMENT ON TABLE api_tokens IS 'API tokens for programmatic access without browser cookies';
COMMENT ON COLUMN api_tokens.token_hash IS 'SHA-256 hash of the token - raw token is never stored';
COMMENT ON COLUMN api_tokens.token_prefix IS 'First 8 characters of raw token for display (e.g., nsk_a1b2)';
COMMENT ON COLUMN api_tokens.scopes IS 'Permission scopes - currently only "full" supported';
COMMENT ON COLUMN api_tokens.created_by IS 'Admin who created this token';
