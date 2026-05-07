-- CSP violation reports collected from browsers via the
-- `report-uri` directive. Browsers POST a JSON report when a
-- subresource load or eval-style operation is blocked by the
-- effective Content-Security-Policy.
--
-- Reports are deduplicated by `dedup_hash` (SHA-256 of the tuple
-- the application considers identifying: effective directive,
-- blocked URI, source file, line number). Identical reports
-- increment `occurrence_count` and bump `last_seen_at` rather than
-- inserting new rows — without this a single broken page reload
-- could blow up the table by hundreds of rows in a few seconds.
--
-- The table is unbounded by design at the schema level; a
-- scheduled prune job in the application removes rows whose
-- `last_seen_at` is older than the retention window (default
-- 30 days). The prune is decoupled from the schema so retention
-- policy is configurable without a migration.

CREATE TABLE csp_reports (
    id                      BIGSERIAL PRIMARY KEY,

    -- Identifying tuple. Hashed at insert to fit a single
    -- UNIQUE INDEX; the source columns are still kept for
    -- display.
    dedup_hash              CHAR(64) NOT NULL,
    effective_directive     VARCHAR(64) NOT NULL,
    blocked_uri             TEXT,
    source_file             TEXT,
    line_number             INTEGER,
    column_number           INTEGER,

    -- Context columns. Useful when investigating but not
    -- part of the dedup tuple — same violation triggered from
    -- two pages still gets one row, just with whichever
    -- document_uri was seen most recently.
    document_uri            TEXT NOT NULL,
    referrer                TEXT,
    violated_directive      VARCHAR(64),
    original_policy         TEXT,

    -- 'enforce' (the default CSP header) vs 'report' (the
    -- Content-Security-Policy-Report-Only header). Helpful
    -- when staging a tightened policy via report-only mode.
    disposition             VARCHAR(16) NOT NULL,

    user_agent              TEXT,

    -- Optional — only set when the report came in with an
    -- authenticated session cookie. CSP reports are
    -- credentialless by default in modern browsers but legacy
    -- behaviour can include them.
    user_uuid               UUID REFERENCES users(uuid) ON DELETE SET NULL,

    occurrence_count        INTEGER NOT NULL DEFAULT 1,
    first_seen_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX csp_reports_dedup_hash_idx ON csp_reports (dedup_hash);
CREATE INDEX csp_reports_last_seen_at_idx ON csp_reports (last_seen_at DESC);
CREATE INDEX csp_reports_effective_directive_idx ON csp_reports (effective_directive, last_seen_at DESC);
