-- Persist the WebAuthn sign counter and backup-state-change timestamp
-- on `passkey_credentials` so we can do clone detection.
--
-- Background: the 2026-05-01-100000_extract_passkey_credentials
-- migration extracted passkeys from a JSONB blob on `users` into a
-- first-class table, and that header comment said:
--
--   "* lets us add columns (sign counter, AAGUID, attestation
--    details) without rewriting the whole blob"
--
-- ...but the follow-up to actually add the counter column never
-- landed. The counter still lives inside `credential JSONB`, and the
-- post-auth path only updates `last_used_at` — the counter is never
-- written back. That broke WebAuthn's clone-detection property
-- (W3C WebAuthn L3 §6.1.3): the assertion's counter is compared to
-- the stored counter, but if the stored counter never advances past
-- the registration value, every legitimate authentication looks
-- identical to a replay from a cloned authenticator at the same
-- baseline counter. This migration adds the columns; the application
-- code in the same change-set wires the post-auth bump + clone
-- signal emission.
--
-- `sign_count BIGINT` because Postgres has no unsigned 32-bit type
-- and the WebAuthn counter is u32 in the protocol; BIGINT widens
-- without truncation. NOT NULL with DEFAULT 0 so existing rows
-- start at the safe floor (any subsequent assertion's counter must
-- be > 0 to pass the regression check, which matches what the
-- library would have done if it had a real prior baseline).
--
-- `backup_state_changed_at TIMESTAMPTZ` records the moment the
-- credential's backup_state flag flipped (set by the application
-- when it observes a flip in finish_passkey_authentication's
-- AuthenticationResult). Used by the security log to surface
-- potential cross-ecosystem syncs of a passkey.

ALTER TABLE passkey_credentials
    ADD COLUMN sign_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN backup_state_changed_at TIMESTAMPTZ;
