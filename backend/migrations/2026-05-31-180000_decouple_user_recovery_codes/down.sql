-- Reverse the user_recovery_codes extraction. Restore the JSONB
-- column on `users` and backfill from the row-per-code table.
--
-- Heads-up: this loses the `used_at` distinction — once codes
-- come back into the JSONB array, the verify path can no longer
-- tell consumed codes from unused. The down migration therefore
-- backfills only UNUSED codes, treating consumed codes as
-- effectively rotated away.

ALTER TABLE users
    ADD COLUMN mfa_backup_codes JSONB;

-- Group unused hashes per user back into a JSON array.
UPDATE users u
SET mfa_backup_codes = sub.codes
FROM (
    SELECT user_uuid,
           jsonb_agg(code_hash) AS codes
    FROM user_recovery_codes
    WHERE used_at IS NULL
    GROUP BY user_uuid
) sub
WHERE u.uuid = sub.user_uuid;

DROP INDEX user_recovery_codes_unused_by_user;
DROP TABLE user_recovery_codes;
