-- Restore the column as it was in the initial schema: nullable, no default.
ALTER TABLE outbound_emails ADD COLUMN delivered_at timestamp with time zone;
