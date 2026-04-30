-- Phase 2a of the docs/KB redesign: Knowledge Gaps queue.
--
-- Two-table model so every detection mechanism (manual flag in
-- 2a, ticket clusters in 2b, failed searches in 2c, stale docs
-- in 2d, AI-suggested in Phase 3) writes into a uniform shape.
--
--   knowledge_gaps         canonical editorial entity
--   knowledge_gap_signals  raw evidence (polymorphic source)
--
-- The gap aggregates one or more signals; an LLM in Phase 3
-- iterates `WHERE status = 'open'`, drafts content using the
-- signals' source evidence, and a human promotes the draft via
-- Phase 1's existing "Save as doc" flow (which sets resolved_page_id
-- and creates 'resolves' rows in documentation_page_tickets for
-- every ticket-typed signal).

CREATE TABLE knowledge_gaps (
    id           BIGSERIAL    PRIMARY KEY,
    title        TEXT         NOT NULL,
    description  TEXT,
    -- Lifecycle. Constrained text rather than a Postgres enum so
    -- adding a new state ('archived', 'merged') later is a CHECK
    -- update instead of an enum migration.
    status       VARCHAR(32)  NOT NULL DEFAULT 'open'
                 CHECK (status IN ('open', 'drafting', 'resolved', 'dismissed')),
    -- Triager / writer. Null = unassigned.
    assignee_uuid UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    -- Resolution: when a doc closes the gap, link it. The status
    -- transition to 'resolved' is what triggers the cascade that
    -- writes documentation_page_tickets 'resolves' rows for every
    -- ticket-typed signal; the column itself is the durable proof.
    resolved_page_id INTEGER  REFERENCES documentation_pages(id) ON DELETE SET NULL,
    -- Aggregation / ranking. Updated whenever a signal is added,
    -- dismissed, or its confidence changes.
    evidence_count   INTEGER  NOT NULL DEFAULT 0,
    last_evidence_at TIMESTAMPTZ,
    impact_score     INTEGER  NOT NULL DEFAULT 0,
    -- Audit
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    created_by   UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    dismissed_at TIMESTAMPTZ,
    dismissed_by UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    resolved_at  TIMESTAMPTZ
);

-- Hot path: queue view sorts by impact_score within active states.
CREATE INDEX idx_knowledge_gaps_active
    ON knowledge_gaps(status, impact_score DESC, last_evidence_at DESC)
    WHERE status IN ('open', 'drafting');

CREATE TABLE knowledge_gap_signals (
    id          BIGSERIAL    PRIMARY KEY,
    gap_id      BIGINT       NOT NULL REFERENCES knowledge_gaps(id) ON DELETE CASCADE,
    -- Signal type. Drives how `payload` is shaped and how the UI
    -- renders the evidence row. CHECK rather than enum for the
    -- same reason status uses CHECK.
    signal_type VARCHAR(32)  NOT NULL
                CHECK (signal_type IN (
                    'manual_flag',
                    'ticket_cluster',
                    'failed_search',
                    'stale_doc',
                    'ai_suggested'
                )),
    -- Polymorphic source reference. (source_kind, source_ref) is
    -- the dedup key per gap so a single ticket can't be evidenced
    -- twice within the same gap. Cross-gap duplicates are allowed
    -- (the application layer merges/dedups when grouping signals
    -- across gaps in 2b).
    --
    --   source_kind = 'ticket'       -> source_ref = ticket id (text)
    --   source_kind = 'search_query' -> source_ref = the query string
    --   source_kind = 'cluster_key'  -> source_ref = a fingerprint
    --                                   like 'category:5|device:macbook-air-m2'
    --   source_kind = 'page'         -> source_ref = doc page id (text)
    source_kind VARCHAR(32)  NOT NULL,
    source_ref  TEXT         NOT NULL,
    -- Signal-type-specific payload. The Phase 3 LLM reads this
    -- alongside the source row's own data when drafting an
    -- article. Keep it loose (JSONB) so each signal type can carry
    -- whatever Nosdesk-specific context (device model, channel
    -- origin, group-scoped category, assignment rule that fired)
    -- best grounds the eventual draft.
    payload     JSONB        NOT NULL DEFAULT '{}',
    -- Confidence 0-100. Manual flag = 100 (a human said so).
    -- Cluster member = 30 * cluster_size (capped). Failed search =
    -- 10 * query_count. Stale doc = 50.
    confidence  INTEGER      NOT NULL DEFAULT 50
                CHECK (confidence BETWEEN 0 AND 100),
    detected_by UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    detected_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- A signal can be dismissed without dismissing the whole gap
    -- (e.g. an agent unflags one ticket but the cluster still
    -- holds it as evidence). The gap auto-dismisses only when its
    -- last live signal goes; that logic lives in the repository.
    dismissed_at TIMESTAMPTZ,
    dismissed_by UUID        REFERENCES users(uuid) ON DELETE SET NULL,
    UNIQUE (gap_id, source_kind, source_ref)
);

-- Hot path 1: render a gap's signals.
CREATE INDEX idx_kg_signals_gap_id
    ON knowledge_gap_signals(gap_id)
    WHERE dismissed_at IS NULL;

-- Hot path 2: "is there an open gap that already covers this
-- source?" — used by every detector to dedup before creating a
-- new gap.
CREATE INDEX idx_kg_signals_source
    ON knowledge_gap_signals(source_kind, source_ref)
    WHERE dismissed_at IS NULL;
