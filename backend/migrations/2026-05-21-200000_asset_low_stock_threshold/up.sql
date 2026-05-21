-- Adds a per-asset low-stock threshold. When set on a
-- stock-tracked asset (quantity NOT NULL), a current quantity
-- at or below this value is rendered as "low stock" in the UI
-- and emits an asset.low_stock SSE event after each usage
-- decrement that crosses the threshold.
--
-- NULL means "no threshold configured" (the default for every
-- existing asset, including the IT inventory carried over from
-- the device era).

ALTER TABLE assets
  ADD COLUMN low_stock_threshold NUMERIC(12, 3) NULL,
  ADD CONSTRAINT assets_low_stock_threshold_nonneg
    CHECK (low_stock_threshold IS NULL OR low_stock_threshold >= 0);
