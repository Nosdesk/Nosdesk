ALTER TABLE assets
  DROP CONSTRAINT IF EXISTS assets_low_stock_threshold_nonneg,
  DROP COLUMN IF EXISTS low_stock_threshold;
