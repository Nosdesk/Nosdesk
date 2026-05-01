-- Restore the JSONB-blob storage shape and backfill from the
-- normalised table. Lossy in one direction: extra columns we may
-- have added later won't survive the round-trip.

ALTER TABLE users
    ADD COLUMN passkey_credentials JSONB;

UPDATE users u
SET passkey_credentials = jsonb_build_object(
    'credentials',
    COALESCE(
        (
            SELECT jsonb_agg(
                jsonb_build_object(
                    'id', pc.credential_id,
                    'name', pc.name,
                    'credential', pc.credential,
                    'transports', to_jsonb(pc.transports),
                    'backup_eligible', pc.backup_eligible,
                    'backup_state', pc.backup_state,
                    'created_at', pc.created_at,
                    'last_used_at', pc.last_used_at
                )
            )
            FROM passkey_credentials pc
            WHERE pc.user_uuid = u.uuid
        ),
        '[]'::jsonb
    )
)
WHERE EXISTS (SELECT 1 FROM passkey_credentials pc WHERE pc.user_uuid = u.uuid);

DROP INDEX IF EXISTS idx_passkey_credentials_user_uuid;
DROP TABLE passkey_credentials;
