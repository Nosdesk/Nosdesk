-- Webhooks table for external integrations
CREATE TABLE webhooks (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL,
    secret VARCHAR(255) NOT NULL,
    events TEXT[] NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT true,
    headers JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    last_triggered_at TIMESTAMPTZ,
    failure_count INTEGER NOT NULL DEFAULT 0,
    disabled_reason TEXT
);

-- Webhook delivery log for audit trail
CREATE TABLE webhook_deliveries (
    id SERIAL PRIMARY KEY,
    uuid UUID DEFAULT uuid_generate_v7() UNIQUE NOT NULL,
    webhook_id INTEGER NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type VARCHAR(100) NOT NULL,
    payload JSONB NOT NULL,
    request_headers JSONB,
    response_status INTEGER,
    response_body TEXT,
    response_headers JSONB,
    attempt_number INTEGER NOT NULL DEFAULT 1,
    duration_ms INTEGER,
    error_message TEXT,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_retry_at TIMESTAMPTZ
);

-- Indexes for common queries
CREATE INDEX idx_webhooks_enabled ON webhooks(enabled) WHERE enabled = true;
CREATE INDEX idx_webhooks_events ON webhooks USING GIN(events);
CREATE INDEX idx_webhook_deliveries_webhook_id ON webhook_deliveries(webhook_id);
CREATE INDEX idx_webhook_deliveries_created_at ON webhook_deliveries(created_at DESC);
CREATE INDEX idx_webhook_deliveries_next_retry ON webhook_deliveries(next_retry_at)
    WHERE next_retry_at IS NOT NULL AND delivered_at IS NULL;

-- Auto-update updated_at timestamp
SELECT diesel_manage_updated_at('webhooks');

COMMENT ON TABLE webhooks IS 'Webhook configurations for external integrations';
COMMENT ON TABLE webhook_deliveries IS 'Log of webhook delivery attempts';
COMMENT ON COLUMN webhooks.events IS 'Array of event types to subscribe to (e.g., ticket.created, comment.added)';
COMMENT ON COLUMN webhooks.failure_count IS 'Consecutive failure count - webhook auto-disabled at 10';
