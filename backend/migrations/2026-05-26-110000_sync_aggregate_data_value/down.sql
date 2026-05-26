-- Postgres cannot drop a value from an enum type. No-op down; the
-- 'data' value is harmless if unused.
SELECT 1;
