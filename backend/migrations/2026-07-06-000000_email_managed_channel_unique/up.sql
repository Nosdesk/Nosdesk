-- At most one `email_managed` channel per workspace: the managed default
-- address `support@<slug>.<tenant_domain>` routes by slug alone, so a second
-- channel row would be unreachable and the inbound webhook's lazy
-- find-or-create could otherwise mint duplicates under a concurrent-delivery
-- race. Partial, so other providers (multi-mailbox IMAP, multiple forwarding
-- addresses) stay unconstrained.
CREATE UNIQUE INDEX channels_one_email_managed_per_workspace
    ON channels (workspace_id)
    WHERE provider = 'email_managed';
