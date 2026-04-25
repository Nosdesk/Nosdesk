-- Move plugin bundle.js from the on-disk uploads volume into a
-- BYTEA column on the `plugins` row. Collapses the previous two-
-- store install (DB row + filesystem write) into a single
-- transactional write, eliminating torn writes between the row
-- and the bundle file. Bundles are small (typical 50-300 KB,
-- capped at 500 KB by `install::MAX_BUNDLE_SIZE`); BYTEA storage
-- is fine at this scale.
--
-- New rows always populate `bundle_js`; existing rows from before
-- this migration retain NULL until they're reinstalled, at which
-- point the install pipeline writes the bytes inline.
ALTER TABLE plugins
    ADD COLUMN bundle_js BYTEA;
