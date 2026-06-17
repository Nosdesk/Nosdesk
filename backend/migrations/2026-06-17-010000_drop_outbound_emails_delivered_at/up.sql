-- `outbound_emails.delivered_at` is dead weight: SMTP gives no delivery
-- confirmation, and the only integration that ever wrote it (the provider
-- delivery webhook) was removed, so the column has only ever been NULL. Drop it
-- rather than ship a column that implies delivery tracking we don't have;
-- `status = 'sent'` (relay accepted handoff) remains the strongest send signal.
--
-- Pure DDL: no row writes, so the outbound_emails audit trigger doesn't fire and
-- needs no disabling.
ALTER TABLE outbound_emails DROP COLUMN delivered_at;
