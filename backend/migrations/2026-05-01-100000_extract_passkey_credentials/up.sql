-- Extract passkey credentials from users.passkey_credentials JSONB into
-- a first-class table. The JSONB-blob design forced full sequential
-- scans on every login (no GIN index) and made adding/removing a
-- passkey a read-modify-write of the whole array, which loses
-- concurrent updates. The new table:
--
--   * indexes credential_id (UNIQUE) so login lookup is O(log n)
--   * makes every credential a row, so add/remove/touch don't
--     race each other
--   * carries a real FK to users with ON DELETE CASCADE
--   * lets us add columns (sign counter, AAGUID, attestation
--     details) without rewriting the whole blob

CREATE TABLE passkey_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    name VARCHAR(100) NOT NULL,
    credential JSONB NOT NULL,
    transports TEXT[] NOT NULL DEFAULT '{}',
    backup_eligible BOOLEAN NOT NULL DEFAULT FALSE,
    backup_state BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

CREATE INDEX idx_passkey_credentials_user_uuid
    ON passkey_credentials(user_uuid);

-- Backfill from the JSONB blob. The on-disk shape is
-- { "credentials": [ { id, name, credential, transports,
-- created_at, last_used_at, backup_eligible, backup_state }, ... ] }.
INSERT INTO passkey_credentials (
    user_uuid, credential_id, name, credential, transports,
    backup_eligible, backup_state, created_at, last_used_at
)
SELECT
    u.uuid,
    cred->>'id',
    cred->>'name',
    cred->'credential',
    COALESCE(
        ARRAY(SELECT jsonb_array_elements_text(cred->'transports')),
        '{}'::TEXT[]
    ),
    COALESCE((cred->>'backup_eligible')::BOOLEAN, FALSE),
    COALESCE((cred->>'backup_state')::BOOLEAN, FALSE),
    COALESCE((cred->>'created_at')::TIMESTAMPTZ, NOW()),
    NULLIF(cred->>'last_used_at', '')::TIMESTAMPTZ
FROM users u,
     jsonb_array_elements(u.passkey_credentials->'credentials') AS cred
WHERE u.passkey_credentials IS NOT NULL
  AND jsonb_typeof(u.passkey_credentials->'credentials') = 'array';

ALTER TABLE users DROP COLUMN passkey_credentials;
