-- Postgres has no ALTER TYPE ... DROP VALUE. The down here is a
-- no-op; recovery is "drop the type and recreate it without
-- those variants" which would also drop every column referencing
-- it. Production rollbacks of an enum addition are rare and not
-- worth the operational tax.
SELECT 1;
