-- Single-row table holding the instance's last-known registry
-- snapshot metadata. `publishers_version` and `index_version` are
-- monotonic counters declared in the signed JSON files served by
-- nosdesk.com; the instance refuses snapshots whose versions are
-- lower than what it has seen (anti-rollback for revoked publishers).
CREATE TABLE plugin_registry_state (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    publishers_version   BIGINT NOT NULL DEFAULT 0,
    index_version        BIGINT NOT NULL DEFAULT 0,
    last_fetched_at      TIMESTAMPTZ,
    last_fetch_error     TEXT,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed the row so callers never have to handle a missing-row case.
INSERT INTO plugin_registry_state (id) VALUES (1);
