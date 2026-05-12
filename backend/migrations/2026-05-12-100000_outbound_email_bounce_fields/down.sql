DROP INDEX IF EXISTS outbound_emails_bounced_idx;
ALTER TABLE outbound_emails
    DROP COLUMN IF EXISTS bounce_diagnostic,
    DROP COLUMN IF EXISTS bounce_recipient,
    DROP COLUMN IF EXISTS bounced_at;
