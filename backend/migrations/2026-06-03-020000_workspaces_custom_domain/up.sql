-- =====================================================================
-- Add `custom_domain` to workspaces (M5 product-side handoff Task 5).
--
-- Hosted-mode tenants in the Standard tier can map a customer-owned
-- domain (e.g. `support.acme.com`) to their workspace. The control
-- plane manages the Fly Certs lifecycle + DNS verification; this
-- column is the product's lookup table from incoming Host header to
-- workspace.
--
-- Why on `workspaces` rather than a separate `custom_domains` table:
-- one-to-one with workspaces today (no plan for multiple custom
-- domains per workspace in v1), tiny per-row payload, and the
-- existing workspace lookup cache can just gain a second key shape.
-- If we ever need many-to-one we promote it to a side table.
--
-- UNIQUE on the column so the same hostname can't point at two
-- workspaces. NULL is the default and the common case (most
-- workspaces use the `<slug>.nosdesk.app` subdomain).
-- =====================================================================

ALTER TABLE workspaces
    ADD COLUMN custom_domain TEXT UNIQUE;
