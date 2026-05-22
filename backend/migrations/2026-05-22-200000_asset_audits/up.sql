-- Stock-count audit ledger. Distinct from asset_usage_log
-- because audits record a *state assertion* ("I counted N
-- units on hand"), not a transactional delta. The system's
-- quantity gets corrected to match the counted value in the
-- same transaction; the audit row captures the previous
-- quantity + the delta so the admin can answer "what was the
-- discrepancy between book and physical stock at audit time"
-- without reconstructing it from history.
--
-- counted_quantity is the ground truth from the physical
-- count. previous_quantity is what assets.quantity held at
-- audit time. delta = counted - previous (signed; positive
-- means "found more than the books showed", negative means
-- "missing"). All three columns are NOT NULL so the row is
-- self-contained for reporting; no joins needed to interpret
-- the audit history.

CREATE TABLE asset_audits (
    id                BIGSERIAL PRIMARY KEY,
    asset_id          INT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    counted_quantity  NUMERIC(12, 3) NOT NULL CHECK (counted_quantity >= 0),
    previous_quantity NUMERIC(12, 3) NOT NULL,
    delta             NUMERIC(12, 3) NOT NULL,
    notes             TEXT,
    recorded_by       UUID REFERENCES users(uuid) ON DELETE SET NULL,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_asset_audits_asset
    ON asset_audits (asset_id, recorded_at DESC);

CREATE TRIGGER tr_audit_asset_audits
    AFTER INSERT OR UPDATE OR DELETE ON asset_audits
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
