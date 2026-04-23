-- Per-user dashboard layout.
--
-- Stores an ordered list of widget ids + visibility flags so that
-- user-driven reorder / show-hide customisation persists across
-- sessions and devices. NULL means "use the role default layout
-- derived from the client-side widget registry."
--
-- Shape (client-authoritative, validated by the update handler):
--   { "widgets": [{ "id": "...", "visible": true }, ...] }
ALTER TABLE users ADD COLUMN dashboard_layout JSONB;
