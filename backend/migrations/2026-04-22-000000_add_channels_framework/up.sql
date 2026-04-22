-- Multi-channel message ingestion framework. Phase 1 implements the
-- `email_imap` provider; the tables and column shapes accommodate future
-- Slack / Teams / Discord / webhook-based providers without migration.

-- One row per configured channel instance (e.g. one IMAP mailbox; later
-- one Slack workspace, one Teams tenant, etc.).
CREATE TABLE channels (
    id             SERIAL      PRIMARY KEY,
    provider       VARCHAR(64) NOT NULL,
    name           VARCHAR(255) NOT NULL,
    enabled        BOOLEAN     NOT NULL DEFAULT true,
    -- Provider-specific configuration (IMAP host/port/user, SMTP from
    -- address, Slack workspace id, etc.). Passwords and tokens do NOT
    -- go here — see channel_credentials.
    config         JSONB       NOT NULL,
    -- Adapter-owned runtime state: IMAP last_seen_uid, Slack webhook
    -- signing secret fingerprint, Teams subscription expiry, Discord
    -- session resume token, etc. Named `runtime_state` (not `poll_state`)
    -- because push/stream adapters will use this too.
    runtime_state  JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_polled_at TIMESTAMPTZ
);

SELECT diesel_manage_updated_at('channels');

CREATE INDEX idx_channels_enabled_provider
    ON channels(provider)
    WHERE enabled = true;

-- Encrypted secrets for a channel (IMAP password, OAuth access/refresh
-- tokens, webhook signing secrets). Values are AES-256-GCM encrypted via
-- utils::encryption keyed on ENCRYPTION_KEY env var. Never stored plain.
CREATE TABLE channel_credentials (
    id              SERIAL      PRIMARY KEY,
    channel_id      INT         NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    credential_type VARCHAR(64) NOT NULL,
    encrypted_value TEXT        NOT NULL,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (channel_id, credential_type)
);

-- Ledger of every message that moved through a channel. Primary uses:
--   1. Dedup — same external_id won't be ingested twice.
--   2. Thread resolution — inbound In-Reply-To / References chains look
--      up parent message external_ids to find the ticket.
--   3. Audit — raw_metadata preserves provider payloads.
CREATE TABLE channel_messages (
    id               BIGSERIAL   PRIMARY KEY,
    channel_id       INT         NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    -- RFC 5322 max Message-ID length is 998 chars. Wide enough for Slack
    -- ts (float timestamp), Teams message id, Discord snowflake, etc.
    external_id      VARCHAR(998) NOT NULL,
    direction        VARCHAR(16)  NOT NULL CHECK (direction IN ('inbound', 'outbound')),
    ticket_id        INT          REFERENCES tickets(id)  ON DELETE SET NULL,
    comment_id       INT          REFERENCES comments(id) ON DELETE SET NULL,
    in_reply_to      VARCHAR(998),
    from_address     VARCHAR(320),
    -- When the sender maps to a known Nosdesk user, store it. Distinguishes
    -- "tech replied out-of-band in Slack" (author_user_uuid set) from
    -- "customer messaged us" (null). Set by the identity resolver.
    author_user_uuid UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    raw_metadata     JSONB,
    received_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (channel_id, external_id, direction)
);

CREATE INDEX idx_channel_messages_external_id ON channel_messages(external_id);
CREATE INDEX idx_channel_messages_ticket_id   ON channel_messages(ticket_id);

-- Link a ticket back to the channel it came in through. Used on outbound
-- relay (tech reply → channel) and for the UI "originated from X" badge.
ALTER TABLE tickets
    ADD COLUMN origin_channel_id INT REFERENCES channels(id) ON DELETE SET NULL;

CREATE INDEX idx_tickets_origin_channel_id
    ON tickets(origin_channel_id)
    WHERE origin_channel_id IS NOT NULL;

-- Per-comment channel metadata (Message-ID we emitted, Slack thread_ts,
-- Discord message id, etc.). Free-form because each provider shapes it
-- differently; we look things up via channel_messages for queries.
ALTER TABLE comments
    ADD COLUMN channel_metadata JSONB;

-- Internal vs. public comments. `true` means tech-to-tech only: NOT shown
-- to requesters in their portal view and NOT relayed back through the
-- originating channel. Default false preserves existing comment semantics.
ALTER TABLE comments
    ADD COLUMN is_internal BOOLEAN NOT NULL DEFAULT false;

-- Soft-delete so future channel-edit/delete events (Slack, Teams) can
-- mark a comment deleted without losing its place in the thread. Phase 1
-- pipeline doesn't yet set this; the column is here so the schema is
-- ready when the pipeline does.
ALTER TABLE comments
    ADD COLUMN deleted_at TIMESTAMPTZ;

CREATE INDEX idx_comments_ticket_id_not_deleted
    ON comments(ticket_id)
    WHERE deleted_at IS NULL;
