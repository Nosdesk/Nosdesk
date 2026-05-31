-- Decouple MFA recovery codes from the JSONB array on `users` into
-- a first-class `user_recovery_codes` table. The JSONB-blob design
-- forced three operational footguns at once:
--
--   1. **Atomicity.** Consuming one code required read array →
--      bcrypt-verify each entry in the app → splice out matched
--      entry → write whole array back. Two concurrent recovery
--      attempts race for the row lock; the second sees a stale
--      array and last-write-wins on the splice loses a consumption.
--   2. **Query cleanliness.** "Which users are running low on
--      recovery codes?" needed `jsonb_array_length` per row — full
--      table scan + parse, can't be indexed. The verify path needed
--      to load the WHOLE array even when only one match was needed.
--   3. **Forced rewrites.** Adding `used_at`, rotation timestamps,
--      or any per-code metadata required schema bumps inside the
--      JSONB shape — every consumer had to know the on-disk shape.
--
-- The row-per-code model fixes all three: single-statement atomic
-- consumption via `UPDATE … WHERE used_at IS NULL`, indexed
-- per-user count queries through the partial index below, and any
-- future per-code metadata is a normal `ALTER TABLE … ADD COLUMN`.
--
-- See `docs/auth-convergence.md` §"Changes Nosdesk should adopt"
-- item 4 for the full rationale (with comparable nosdesk-com
-- implementation reference).
--
-- Pairs naturally with the F2C.3 M4 constant-time-verify finding:
-- the verify path is being rewritten as part of this change-set,
-- so the early-return-on-match timing leak is fixed in the same
-- pass.

CREATE TABLE user_recovery_codes (
    id          BIGSERIAL    PRIMARY KEY,
    user_uuid   UUID         NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    code_hash   TEXT         NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Partial index on the hot filter. Verify path runs `SELECT … WHERE
-- user_uuid = $1 AND used_at IS NULL`; this index covers exactly
-- that shape and stays small as codes are consumed (a user who has
-- burned 7 of 10 codes sees 3 entries here vs 10 entries in the
-- previous JSONB array).
CREATE INDEX user_recovery_codes_unused_by_user
    ON user_recovery_codes (user_uuid)
    WHERE used_at IS NULL;

-- Backfill from existing JSONB arrays. Each non-null string entry
-- in `users.mfa_backup_codes` becomes one row in the new table.
-- bcrypt hashes are opaque strings so the migration moves them
-- verbatim — no re-hashing needed, no per-row work beyond the
-- INSERT … SELECT.
INSERT INTO user_recovery_codes (user_uuid, code_hash)
SELECT u.uuid, jsonb_array_elements_text(u.mfa_backup_codes)
FROM users u
WHERE u.mfa_backup_codes IS NOT NULL
  AND jsonb_typeof(u.mfa_backup_codes) = 'array';

-- Retire the JSONB column. The application code in this
-- change-set drops every read/write of users.mfa_backup_codes,
-- so the column has no consumers post-deploy. If you're staging
-- the migration ahead of the code deploy (rare for a self-host
-- single-tenant app), comment this out and run a follow-up
-- migration after the code lands.
ALTER TABLE users DROP COLUMN mfa_backup_codes;
