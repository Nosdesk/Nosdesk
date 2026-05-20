-- Generalise the devices domain into a runtime-extensible assets
-- domain. The previous shape was "one devices table plus a
-- pretend assets nav entry that pointed at the same rows"; nobody
-- in the industry actually models it that way (Snipe-IT, GLPI,
-- Freshservice, HaloITSM all use a single asset table with a
-- kind discriminator).
--
-- This migration is a metadata-only Postgres operation: RENAME
-- TABLE / RENAME COLUMN doesn't rewrite rows, indexes follow the
-- table automatically, foreign key references update in place.
-- We rename the index identifiers for clarity but the underlying
-- B-trees stay where they are.
--
-- Existing rows backfill to `kind = 'device'` so every consumer
-- that used to hit `/devices/*` continues to see exactly the
-- same data; the discriminator only matters when new kinds get
-- created via the asset_kinds registry.

-- Table renames ----------------------------------------------------------

ALTER TABLE devices RENAME TO assets;
ALTER TABLE device_groups RENAME TO asset_groups;
ALTER TABLE ticket_devices RENAME TO ticket_assets;

ALTER TABLE asset_groups RENAME COLUMN device_id TO asset_id;
ALTER TABLE ticket_assets RENAME COLUMN device_id TO asset_id;

-- Index renames (cosmetic; the indexes work either way, but
-- `idx_devices_*` is misleading once the table is `assets`).
ALTER INDEX idx_device_serial_unique RENAME TO idx_asset_serial_unique;
ALTER INDEX idx_devices_primary_user RENAME TO idx_assets_primary_user;
ALTER INDEX idx_devices_serial_number RENAME TO idx_assets_serial_number;
ALTER INDEX idx_devices_created_at RENAME TO idx_assets_created_at;
ALTER INDEX idx_devices_warranty_end_date RENAME TO idx_assets_warranty_end_date;
ALTER INDEX idx_devices_asset_tag RENAME TO idx_assets_asset_tag;
ALTER INDEX idx_device_groups_device RENAME TO idx_asset_groups_asset;
ALTER INDEX idx_device_groups_group RENAME TO idx_asset_groups_group;
ALTER INDEX idx_device_groups_external RENAME TO idx_asset_groups_external;

-- New columns on assets --------------------------------------------------

-- Discriminator. Defaults to 'device' so every pre-existing row
-- lands in the IT-desk view by definition. New kinds register
-- their slug in `asset_kinds` and rows reference it by string.
ALTER TABLE assets
    ADD COLUMN kind VARCHAR(64) NOT NULL DEFAULT 'device';

-- Type-specific fields. The core columns (name, serial, owner,
-- warranty_*, purchase_date) stay structured because every kind
-- needs them; truly per-kind fields (a pipe's diameter, a
-- license's seat count) live here. Validated at write time
-- against the kind's JSON Schema in `asset_kinds.attribute_schema`.
ALTER TABLE assets
    ADD COLUMN attributes JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Consumables / materials use these; one laptop is one row, but
-- a roll of 22mm copper pipe is `quantity = 50, unit = 'm'`.
-- Nullable so IT-desk kinds aren't forced to set them.
ALTER TABLE assets
    ADD COLUMN quantity NUMERIC(12, 3);
ALTER TABLE assets
    ADD COLUMN unit VARCHAR(32);

-- Common lookup paths. `kind` is hit by every list view (one
-- filter per kind chip); a plain btree is enough for the cardinality
-- a registry of ~10–50 kinds will ever hit.
CREATE INDEX idx_assets_kind ON assets (kind);

-- Runtime kind registry --------------------------------------------------

-- Admins define new kinds here. `slug` is the FK-style identifier
-- assets reference. `attribute_schema` is a constrained JSON
-- Schema subset (root object, primitive properties with
-- enum/min/max/length/pattern/format, required array); the
-- backend validates submitted asset.attributes against it on
-- create / update.
CREATE TABLE asset_kinds (
    id              SERIAL PRIMARY KEY,
    slug            VARCHAR(64) NOT NULL UNIQUE,
    label           VARCHAR(255) NOT NULL,
    description     TEXT,
    -- Heroicon name the frontend resolves; null falls back to a
    -- generic asset glyph.
    icon            VARCHAR(64),
    -- JSON Schema subset: see services::assets::kinds for the
    -- validator. Empty object `{"type":"object","properties":{}}`
    -- means "no kind-specific attributes."
    attribute_schema JSONB NOT NULL DEFAULT '{"type":"object","properties":{}}'::jsonb,
    -- Sort order in admin pickers and the navbar chip row.
    sort_order      INTEGER NOT NULL DEFAULT 100,
    -- Built-ins ship with the product and can't be deleted by an
    -- admin (you can edit the attribute_schema but not the slug).
    is_builtin      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by      UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Seed the built-in kinds. The IT-desk set keeps the existing
-- experience: an admin who never touches the kinds registry sees
-- the same Devices view they had before, just with a cleaner
-- backing schema. The non-IT seeds (vehicle, equipment,
-- consumable, material, license) are starting points the plumber-
-- style use case extends from.
INSERT INTO asset_kinds (slug, label, description, icon, sort_order, is_builtin) VALUES
    ('device',          'Device',          'Generic IT device. Default kind for assets created via the legacy /devices path.', 'device', 10, TRUE),
    ('laptop',          'Laptop',          'Portable computer assigned to a user.', 'laptop', 20, TRUE),
    ('desktop',         'Desktop',         'Workstation computer at a fixed location.', 'desktop', 30, TRUE),
    ('server',          'Server',          'Server hardware in a data centre or office.', 'server', 40, TRUE),
    ('phone',           'Phone',           'Mobile phone or VoIP handset.', 'phone', 50, TRUE),
    ('monitor',         'Monitor',         'External display.', 'monitor', 60, TRUE),
    ('network_device',  'Network device',  'Switch, router, access point, firewall.', 'network', 70, TRUE),
    ('license',         'License',         'Software license with optional seat tracking.', 'license', 80, TRUE),
    ('vehicle',         'Vehicle',         'Car, van, truck, trailer.', 'vehicle', 90, TRUE),
    ('equipment',       'Equipment',       'Tools, machinery, instruments.', 'equipment', 100, TRUE),
    ('consumable',      'Consumable',      'Items consumed during work (uses quantity + unit).', 'consumable', 110, TRUE),
    ('material',        'Material',        'Bulk material tracked by quantity (pipe lengths, cable rolls).', 'material', 120, TRUE);
