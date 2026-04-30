-- Phase 2c of the docs/KB redesign: failed-search signals.
--
-- Append-only log of doc-search queries with their result count.
-- A scheduled aggregator (run_failed_search_detection) groups
-- zero-result rows by `query_norm`, finds queries that recur
-- N+ times in a window, and writes failed_search signals on
-- knowledge_gaps via the same path 2a/2b use.
--
-- Privacy: no user_uuid column. The query string itself is
-- enough for the detector and aggregation; tying it to a user
-- adds a "who searched what" surface area we don't need. A
-- scheduled cleanup (Phase 2d work) drops rows older than
-- 90 days to keep the log bounded.

CREATE TABLE search_query_log (
    id            BIGSERIAL    PRIMARY KEY,
    query_raw     TEXT         NOT NULL,
    -- Lower-cased, whitespace-collapsed query used as the
    -- aggregation key. The detector groups by this, not the raw
    -- form; stemming/synonyms happen inside the search engine
    -- before it returns zero, so by the time we log we don't
    -- need to do it again.
    query_norm    TEXT         NOT NULL,
    result_count  INTEGER      NOT NULL,
    searched_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Hot path: detector reads zero-result rows grouped by query_norm
-- within a recent window. Partial index keeps the index small
-- since most searches succeed.
CREATE INDEX idx_search_query_log_failed
    ON search_query_log(query_norm, searched_at)
    WHERE result_count = 0;

-- Retention sweep: drop everything older than the configured
-- window. Plain index on searched_at supports the range delete.
CREATE INDEX idx_search_query_log_searched_at
    ON search_query_log(searched_at);
