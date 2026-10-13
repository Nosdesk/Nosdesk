-- What requester-role members may see of the workspace's assets: `all` (every
-- asset, as before) or `none`. A later `groups` mode (chosen asset groups)
-- takes its place beside these. Adding a column with a constant default
-- rewrites no rows, so the site_settings audit trigger doesn't fire.
ALTER TABLE public.site_settings
    ADD COLUMN requester_asset_visibility text NOT NULL DEFAULT 'all'
        CONSTRAINT site_settings_requester_asset_visibility_check
        CHECK (requester_asset_visibility IN ('all', 'none'));
