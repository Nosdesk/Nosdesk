-- Scope the email suppression list per workspace.
--
-- The table was keyed on the address alone with no `workspace_id`, so the RLS
-- GUC that `TenantConn` primes was a no-op for it and the three admin
-- endpoints had no tenant predicate. On a multi-workspace deployment that let
-- any workspace admin read, add to, and delete from every other tenant's
-- suppression list. Reading it is the sharper half: the module's own doc
-- withholds the list from agents precisely because it reveals email addresses
-- they should not know, and a peer tenant knows them less.
--
-- Suppression is a property of a RELATIONSHIP, not of an address. A manual
-- entry records that this workspace was asked to stop writing to someone, and
-- deleting a peer's entry could resume mail to a person who opted out. Shared
-- sender reputation is the ESP's concern, and SES keeps its own account-level
-- suppression list for exactly that.
--
-- No audit trigger on this table, so the backfill needs no DISABLE TRIGGER
-- dance (the DISABLE/ENABLE pair around it in the initial schema is a
-- seed-load artefact). It is not a partitioned table either.

ALTER TABLE email_suppressions ADD COLUMN workspace_id INTEGER;

-- The address is only unique within a workspace now, so two tenants can each
-- suppress the same person independently. The address-only key goes before
-- the backfill, which copies a row for each further workspace that mailed the
-- address; the per-workspace key goes on once every row has its workspace.
ALTER TABLE email_suppressions DROP CONSTRAINT email_suppressions_pkey;

DO $$
DECLARE
    ws_count INTEGER;
    sole_ws  INTEGER;
    orphaned INTEGER;
BEGIN
    SELECT count(*) INTO ws_count FROM workspaces;

    IF ws_count <= 1 THEN
        -- Every self-hosted Community build is here: one workspace, so every
        -- existing suppression is unambiguously its own and nothing is lost.
        SELECT id INTO sole_ws FROM workspaces LIMIT 1;
        UPDATE email_suppressions SET workspace_id = sole_ws;
    ELSE
        -- Attribute each suppression to the workspaces that actually mailed
        -- that address. Deriving it from evidence rather than assigning
        -- everything to one workspace matters here: copying a row into a
        -- workspace that never wrote to the address would hand that tenant an
        -- email address it has no relationship with, which is the leak this
        -- migration exists to close.
        --
        -- One row per (workspace, address) pair. The first is claimed by
        -- UPDATE; any others are inserted as copies, since two tenants can
        -- each legitimately have hard-bounced the same address.
        UPDATE email_suppressions es
           SET workspace_id = sub.workspace_id
          FROM (
            SELECT lower(oe.recipient) AS email,
                   min(oe.workspace_id) AS workspace_id
              FROM outbound_emails oe
             GROUP BY lower(oe.recipient)
          ) sub
         WHERE sub.email = es.email;

        INSERT INTO email_suppressions
            (email, reason, bounce_diagnostic, bounce_count, created_at, last_seen_at, metadata, workspace_id)
        SELECT es.email, es.reason, es.bounce_diagnostic, es.bounce_count,
               es.created_at, es.last_seen_at, es.metadata, others.workspace_id
          FROM email_suppressions es
          JOIN (
            SELECT DISTINCT lower(oe.recipient) AS email, oe.workspace_id
              FROM outbound_emails oe
          ) others ON others.email = es.email
         WHERE es.workspace_id IS NOT NULL
           AND others.workspace_id <> es.workspace_id;
    END IF;

    -- Anything still unattributed was never sent to by any workspace this
    -- database still has history for. Dropping it is the only non-leaking
    -- option, and it self-heals: the next hard bounce re-suppresses, for the
    -- workspace that actually sent. Report the count so an operator reading
    -- the migration log knows what went.
    SELECT count(*) INTO orphaned FROM email_suppressions WHERE workspace_id IS NULL;
    IF orphaned > 0 THEN
        RAISE NOTICE 'email_suppressions: dropping % row(s) that no workspace has outbound history for', orphaned;
        DELETE FROM email_suppressions WHERE workspace_id IS NULL;
    END IF;
END $$;

ALTER TABLE email_suppressions ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE email_suppressions
    ADD CONSTRAINT email_suppressions_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE;

ALTER TABLE email_suppressions ADD PRIMARY KEY (workspace_id, email);

ALTER TABLE email_suppressions ENABLE ROW LEVEL SECURITY;
ALTER TABLE email_suppressions FORCE ROW LEVEL SECURITY;
CREATE POLICY email_suppressions_workspace_isolation ON email_suppressions
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
