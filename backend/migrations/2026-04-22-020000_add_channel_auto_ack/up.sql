-- Site-wide auto-acknowledgement on channel-opened tickets.
--
-- When enabled (default), the channel pipeline sends a single
-- system-authored reply back to the customer the moment a new
-- ticket opens — "Thanks, ref #N, reply to this email to add
-- details." Standard helpdesk behaviour (Zendesk, Freshdesk,
-- Help Scout all ship it on by default).
--
-- The template is rendered with `{{ticket_id}}`, `{{ticket_title}}`,
-- `{{customer_name}}`, and `{{app_name}}` substitutions. NULL here
-- means "use the built-in default" — admins only store a row when
-- they want to override the wording.

ALTER TABLE site_settings
    ADD COLUMN channel_auto_ack_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN channel_auto_ack_template TEXT;
