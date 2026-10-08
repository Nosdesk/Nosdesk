-- The composer's id for a reply, so a resend of the same reply (a retry after
-- a failed send keeps it) is answered with the comment already saved instead
-- of a second one. Scoped to the ticket and the author; NULL for replies from
-- anywhere else. No backfill.
ALTER TABLE public.comments ADD COLUMN client_id uuid;

CREATE UNIQUE INDEX comments_ticket_author_client_id_key
    ON public.comments (workspace_id, ticket_id, user_uuid, client_id)
    WHERE client_id IS NOT NULL;
