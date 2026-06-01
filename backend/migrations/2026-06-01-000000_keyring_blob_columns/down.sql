-- Reverse of up.sql. Restores the legacy single-key TEXT/hex shape.
-- Like the forward direction, this drops ciphertext rather than
-- attempting a reverse data migration.

-- users.mfa_secret: BYTEA -> VARCHAR
DROP INDEX IF EXISTS users_mfa_secret_kek_id_idx;
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_mfa_secret_kek_id_present_iff_secret;
UPDATE users SET mfa_enabled = false WHERE mfa_enabled = true;
ALTER TABLE users
    DROP COLUMN mfa_secret_kek_id,
    DROP COLUMN mfa_secret;
ALTER TABLE users ADD COLUMN mfa_secret VARCHAR;

-- channel_credentials.encrypted_value: BYTEA -> TEXT
DROP INDEX IF EXISTS channel_credentials_kek_id_idx;
TRUNCATE TABLE channel_credentials;
ALTER TABLE channel_credentials
    DROP COLUMN encrypted_kek_id,
    DROP COLUMN encrypted_value;
ALTER TABLE channel_credentials ADD COLUMN encrypted_value TEXT NOT NULL;

-- plugin_local_signing_key: drop sidecar
DROP INDEX IF EXISTS plugin_local_signing_key_kek_id_idx;
DELETE FROM plugin_local_signing_key;
ALTER TABLE plugin_local_signing_key DROP COLUMN encrypted_sk_kek_id;

-- plugin_data secret settings: nothing to reverse (no schema change).
