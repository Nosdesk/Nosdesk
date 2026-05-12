-- Link DSN bounces back to the originating outbound row.
--
-- A bounce DSN arrives via the inbound IMAP pipeline (detected by
-- `services::channels::email_imap::detect_bounce` in J Pass 2.1)
-- and carries the original Message-ID we stamped on send. We use
-- that to find the matching outbound row and stamp these three
-- fields:
--
--   bounced_at         when the bounce DSN was processed
--   bounce_recipient   the address that actually failed (may
--                      differ from `to_addr` if the recipient was
--                      part of a list expansion or forward)
--   bounce_diagnostic  raw RFC 3464 Diagnostic-Code or Status text
--                      so the admin queue view can show *why*
--
-- All three are NULL for the normal lifecycle (pending → sent).
-- A non-null `bounced_at` is the canonical "this row bounced"
-- signal and drives the admin UI badge. The `failed` / `dead`
-- states remain SMTP-attempt outcomes; bounces are a separate
-- delivery-result axis recorded alongside, not in place of.
--
-- Partial index on bounced_at because the admin queue often
-- filters to "show me bounces" — full scans would otherwise touch
-- millions of `sent` rows where bounced_at IS NULL.

ALTER TABLE outbound_emails
    ADD COLUMN bounced_at        TIMESTAMPTZ NULL,
    ADD COLUMN bounce_recipient  TEXT        NULL,
    ADD COLUMN bounce_diagnostic TEXT        NULL;

CREATE INDEX outbound_emails_bounced_idx
    ON outbound_emails (bounced_at DESC)
    WHERE bounced_at IS NOT NULL;
