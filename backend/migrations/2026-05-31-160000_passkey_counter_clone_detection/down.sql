ALTER TABLE passkey_credentials
    DROP COLUMN sign_count,
    DROP COLUMN backup_state_changed_at;
