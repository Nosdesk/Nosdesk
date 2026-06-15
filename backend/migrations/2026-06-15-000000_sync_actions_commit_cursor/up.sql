-- Commit-safe change-feed cursor.
--
-- `sync_id` is a sequence assigned at INSERT time, so its order can
-- diverge from *commit* order: a transaction that grabs a lower sync_id
-- but commits after a higher-sync_id transaction was already drained
-- gets skipped by both the SSE drain and the /delta catch-up (both
-- cursor on `sync_id > from`). Record each row's transaction id so the
-- feed can instead cursor by a commit horizon (pg_snapshot_xmin): a row
-- is delivered only once its xid is below the horizon (definitely
-- settled), ordered by (xid8, sync_id). A late-committing lower-sync_id
-- row carries a *higher* xid8 and is delivered next rather than skipped.
--
-- Added on the partitioned parent so it propagates to every existing
-- and future partition (the runtime provisioner uses LIKE INCLUDING ALL).
ALTER TABLE public.sync_actions
    ADD COLUMN xid8 bigint NOT NULL DEFAULT (pg_current_xact_id()::text::bigint);

CREATE INDEX sync_actions_xid8_sync_id_idx ON public.sync_actions (xid8, sync_id);
