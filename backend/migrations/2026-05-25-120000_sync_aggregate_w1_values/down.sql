-- Postgres has no DROP VALUE for enums; removing a value requires
-- recreating the type and rewriting every dependent column, which is
-- destructive and not worth it for a reversible-migration gesture.
-- The added values are inert until a repository emits them, so
-- leaving them in place on a down-migration is harmless. No-op.
SELECT 1;
