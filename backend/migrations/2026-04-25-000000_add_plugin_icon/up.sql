-- Plugin icons. Convention-based: if the signed zip contains
-- `icon.svg` at its root, the backend extracts it during install
-- and stores the bytes here. Served at GET /api/plugins/<uuid>/icon
-- with Content-Type: image/svg+xml.
ALTER TABLE plugins
    ADD COLUMN icon_svg BYTEA;
