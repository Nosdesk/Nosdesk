-- Postgres cannot drop a value from an enum type. Reversing this would
-- require recreating user_role without 'audit_reviewer' and rewriting
-- every dependent column, which is not safe to do automatically. Any
-- users still holding the role would block the type swap. No-op down;
-- the value is harmless if unused.
SELECT 1;
