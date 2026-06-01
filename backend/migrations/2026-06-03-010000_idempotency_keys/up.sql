-- =====================================================================
-- Idempotency-Key storage for control-plane provisioning callbacks
-- (and any future external integration that needs Stripe-style
-- idempotent POST semantics).
--
-- Per M5 product-side handoff Task 2 + the M5.4 transactional-outbox
-- design: the control plane's worker may retry a workspace-create POST
-- after a network failure. Without idempotency, a retry mints a second
-- workspace_uuid; this table lets the second call short-circuit to the
-- first call's cached response so both sides converge on the same row.
--
-- `key` is the caller-supplied `Idempotency-Key` header value, prefixed
-- with the route path by the middleware to scope namespaces (e.g.
-- `POST /api/internal/v1/workspaces/create:provision-<instance_id>`).
-- Storing the prefixed form keeps two routes from colliding on the
-- same caller-side key.
--
-- `response_body` is JSONB so we can short-circuit handlers that
-- return JSON without re-running them; the response is replayed
-- verbatim including the status code so retries are byte-identical
-- to the first call's response.
--
-- Retention: the scheduled-jobs sweeper drops rows older than 24h.
-- That's well past any reasonable retry window; the control-plane
-- worker either succeeds in seconds-to-minutes or escalates to
-- operator attention.
--
-- Not workspace-scoped: this table is platform-level (control plane
-- writes to it across every workspace). No RLS.
-- =====================================================================

CREATE TABLE idempotency_keys (
    key             TEXT PRIMARY KEY,
    response_body   JSONB NOT NULL,
    response_status SMALLINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sweeper queries: `DELETE ... WHERE created_at < now() - interval '24 hours'`.
-- Index on created_at keeps the scan cheap once the table grows.
CREATE INDEX idempotency_keys_created_at_idx
    ON idempotency_keys (created_at);
