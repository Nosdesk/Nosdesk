-- Stored files of hard-deleted workspaces, waiting to be removed. A workspace's
-- files live under ws/{id}/ in storage, which the database cascade can't reach,
-- so the hard delete queues a row here in the same transaction and a scheduled
-- job deletes the prefix, retrying until it succeeds.
--
-- Not a tenant table: the workspace row is gone by the time this is read, so
-- the id is kept as a plain integer with no foreign key, and there is no RLS.
CREATE TABLE public.workspace_file_purges (
    deleted_workspace_id integer     PRIMARY KEY,
    queued_at            timestamptz NOT NULL DEFAULT now(),
    attempts             integer     NOT NULL DEFAULT 0,
    -- A static error kind, never a message body.
    last_error           text,
    completed_at         timestamptz
);
ALTER TABLE public.workspace_file_purges OWNER TO nosdesk_admin;
