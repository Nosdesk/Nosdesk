-- =====================================================================
-- Convert at-rest encryption to versioned-KEK self-describing framed
-- blobs (auth-convergence.md items 1-3).
--
-- BREAKING PRE-V1 CHANGE: existing ciphertext is dropped, not migrated.
-- The AEAD tag was computed without row-identity AAD; the new shape
-- binds AAD per-row, so a SQL-side header wrap is insufficient. The
-- only correct migrations are (a) decrypt + re-encrypt under new AAD
-- in Rust, or (b) discard and re-collect. We pick (b) for v1.
--
-- Operator impact after this migration:
--   * Every user with MFA enabled must re-enroll their authenticator.
--   * Every channel credential (IMAP password, OAuth refresh token,
--     etc.) must be re-entered.
--   * The plugin local signing key is regenerated on next boot — any
--     installed plugin must be re-trusted against the new fingerprint.
--   * Plugin secret settings are cleared; operators re-enter via the
--     admin UI.
--
-- New env var contract (replaces ENCRYPTION_KEY / MFA_ENCRYPTION_KEY):
--   MFA_KEK_V1       = 64 hex chars (= 32 bytes)
--   MFA_KEK_VERSION  = 1   (omittable while only one key is loaded)
--
-- Blob layout in the new BYTEA columns:
--   byte  0     : version  (0x01 only)
--   byte  1     : alg      (0x01 = AES-256-GCM only)
--   bytes 2..4  : kek_id   (big-endian u16)
--   bytes 4..16 : nonce    (12 bytes)
--   bytes 16..N-16 : ciphertext
--   bytes N-16..N  : auth tag (16 bytes)
-- =====================================================================

-- ----- users.mfa_secret: VARCHAR(hex nonce||ct||tag) -> BYTEA framed -----

-- Force re-enrollment for any user currently enrolled, then drop and
-- re-add the column with the new type. NULLing in place would leave
-- mfa_enabled=true with no secret, which the verify path treats as
-- "MFA misconfigured" rather than "needs setup".
UPDATE users SET mfa_enabled = false WHERE mfa_enabled = true;

ALTER TABLE users DROP COLUMN mfa_secret;
ALTER TABLE users
    ADD COLUMN mfa_secret BYTEA,
    ADD COLUMN mfa_secret_kek_id SMALLINT;

-- Sidecar mirror of the kek_id encoded in the blob. Authoritative copy
-- lives inside the blob; the sidecar exists so the rewrap job can do
-- `WHERE mfa_secret_kek_id < $current` without parsing every blob.
-- The application MUST verify the two agree on read.
ALTER TABLE users
    ADD CONSTRAINT users_mfa_secret_kek_id_present_iff_secret
    CHECK ((mfa_secret IS NULL) = (mfa_secret_kek_id IS NULL));

CREATE INDEX users_mfa_secret_kek_id_idx
    ON users (mfa_secret_kek_id)
    WHERE mfa_secret IS NOT NULL;

-- ----- channel_credentials.encrypted_value: TEXT -> BYTEA framed -----

-- TRUNCATE rather than DROP TABLE: foreign keys (and the audit trigger)
-- stay intact. Empty table lets us re-add as NOT NULL without scanning.
TRUNCATE TABLE channel_credentials;

ALTER TABLE channel_credentials DROP COLUMN encrypted_value;
ALTER TABLE channel_credentials
    ADD COLUMN encrypted_value BYTEA NOT NULL,
    ADD COLUMN encrypted_kek_id SMALLINT NOT NULL;

CREATE INDEX channel_credentials_kek_id_idx
    ON channel_credentials (encrypted_kek_id);

-- ----- plugin_local_signing_key: BYTEA already, just clear + sidecar -----

-- Regenerated automatically by services::plugins::local_key on next
-- boot. Operators of instances with installed plugins must re-trust
-- the new fingerprint (logged at WARN on generation).
DELETE FROM plugin_local_signing_key;

ALTER TABLE plugin_local_signing_key
    ADD COLUMN encrypted_sk_kek_id SMALLINT NOT NULL;

CREATE INDEX plugin_local_signing_key_kek_id_idx
    ON plugin_local_signing_key (encrypted_sk_kek_id);

-- ----- plugin_data secret settings: clear (JSONB stays JSONB) -----

-- Secret plugin settings continue to live in the JSONB `value` column
-- as a hex-encoded framed blob (string scalar). No schema change here;
-- the format change happens in application code. The sidecar pattern
-- doesn't apply since this column is polymorphic across plugins.
DELETE FROM plugin_data WHERE is_secret = true;
