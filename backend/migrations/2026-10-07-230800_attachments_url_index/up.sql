-- A file the browser loads is found by its stored URL (`attachments.url`),
-- which had no index, so every load scanned the table. A hash index: the
-- lookup is equality only, and a hash index has no row-size limit on the
-- long URL column.
--
-- Built inside the migration's transaction, so writes to attachments wait
-- until it commits.
CREATE INDEX IF NOT EXISTS attachments_url_hash_idx ON public.attachments USING hash (url);
