-- Asset-kind category. Drives "show or hide the IT-flavoured
-- columns and the IT planner" decisions in the UI without
-- hard-coding kind slugs into the frontend.
--
-- Categories:
--   it       - IT-managed hardware. Renders hostname, OS, warranty,
--              Intune/Entra, compliance state. Asset planner
--              applies. Built-ins: device, laptop, desktop, server,
--              phone, monitor, network_device.
--   logical  - Non-physical assets where warranty and serial don't
--              apply but seat counts / expiry dates might. Built-ins:
--              license.
--   physical - Tangible, trackable assets without an IT lifecycle.
--              Built-ins: vehicle, equipment.
--   bulk     - Tracked by quantity + unit, not as individual rows.
--              Built-ins: consumable, material.
--   generic  - Workspace-neutral fallback. Renders only the
--              universal core fields.
--
-- Custom admin-created kinds default to 'generic' on insert; the
-- admin UI lets them re-categorise to any of the above. The set
-- is closed (not free-text) so the UI's render logic stays
-- predictable.

ALTER TABLE asset_kinds
    ADD COLUMN category VARCHAR(16) NOT NULL DEFAULT 'generic'
    CHECK (category IN ('it', 'logical', 'physical', 'bulk', 'generic'));

UPDATE asset_kinds SET category = 'it'       WHERE slug IN ('device', 'laptop', 'desktop', 'server', 'phone', 'monitor', 'network_device');
UPDATE asset_kinds SET category = 'logical'  WHERE slug = 'license';
UPDATE asset_kinds SET category = 'physical' WHERE slug IN ('vehicle', 'equipment');
UPDATE asset_kinds SET category = 'bulk'     WHERE slug IN ('consumable', 'material');
-- 'generic' kind already has category='generic' via the column default.
