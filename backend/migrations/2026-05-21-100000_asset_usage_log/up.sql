-- Asset usage ledger. One row per "X units consumed" event.
-- The plumbing-business use case: a technician records "used 5
-- meters of 20mm copper pipe on ticket #142" and the asset's
-- on-hand quantity decrements in the same transaction.
--
-- Gate at the handler boundary: the asset must have
-- `quantity IS NOT NULL` for a usage row to be inserted (i.e.
-- the asset is stock-tracked). Non-stock-tracked assets
-- (laptops, vehicles) don't appear in the usage UI.
--
-- Why nullable `ticket_id`: usage can be ad-hoc (restock
-- audits, write-offs) as well as ticket-driven. Ad-hoc events
-- still need an audit trail.
--
-- Why store `unit` on the row instead of joining through
-- assets.unit at read time: the asset's unit could change
-- later (rare, but the admin form allows it); the ledger row
-- should carry the unit it was recorded in. Same reasoning
-- the `comments.content_format` column stores its format.
--
-- The audit_log trigger is attached so every insert/update/
-- delete leaves a forensic trail; updates are rare (corrections
-- via the API) and deletes shouldn't happen at all in normal
-- operation.

CREATE TABLE asset_usage_log (
    id              BIGSERIAL PRIMARY KEY,
    asset_id        INT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    ticket_id       INT REFERENCES tickets(id) ON DELETE SET NULL,
    quantity_used   NUMERIC(12, 3) NOT NULL CHECK (quantity_used > 0),
    unit            VARCHAR(32) NOT NULL,
    recorded_by     UUID REFERENCES users(uuid) ON DELETE SET NULL,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    notes           TEXT
);

CREATE INDEX idx_asset_usage_log_asset
    ON asset_usage_log (asset_id, recorded_at DESC);

CREATE INDEX idx_asset_usage_log_ticket
    ON asset_usage_log (ticket_id) WHERE ticket_id IS NOT NULL;

CREATE TRIGGER tr_audit_asset_usage_log
    AFTER INSERT OR UPDATE OR DELETE ON asset_usage_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
