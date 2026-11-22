-- The asset timeline reads an asset's linked tickets, and the primary key
-- leads with the ticket. Workspace first, as RLS filters every read by it.
CREATE INDEX ticket_assets_asset_idx
    ON public.ticket_assets (workspace_id, asset_id, ticket_id);
