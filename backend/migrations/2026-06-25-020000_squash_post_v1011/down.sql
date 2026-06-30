-- Reverses the squashed migration (the 18 downs in reverse order).

-- from 2026-06-25-010000_workspace_ldap_sync_state
DROP TABLE IF EXISTS public.workspace_ldap_sync_state;

-- from 2026-06-25-000000_ldap_tls_mode_drop_plain
ALTER TABLE public.workspace_ldap_settings
    DROP CONSTRAINT workspace_ldap_settings_tls_mode_check;
ALTER TABLE public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_tls_mode_check
    CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls', 'plain'])::text[])));

-- from 2026-06-24-030000_workspace_ldap_settings
DROP TABLE IF EXISTS public.workspace_ldap_settings;

-- from 2026-06-24-020000_scope_user_auth_identities
DROP INDEX IF EXISTS public.idx_user_auth_identities_workspace_id;
DROP INDEX IF EXISTS public.user_auth_identities_scoped_uq;
DROP INDEX IF EXISTS public.user_auth_identities_global_uq;
ALTER TABLE public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_provider_type_external_id_key UNIQUE (provider_type, external_id);
ALTER TABLE public.user_auth_identities
    DROP CONSTRAINT user_auth_identities_workspace_id_fkey;
ALTER TABLE public.user_auth_identities
    DROP COLUMN workspace_id;

-- from 2026-06-24-010000_user_contact_satellites
DROP TABLE IF EXISTS public.user_addresses;
DROP TABLE IF EXISTS public.user_phone_numbers;

-- from 2026-06-24-000000_user_contact_fields
DROP TABLE IF EXISTS public.user_profiles;
DROP TABLE IF EXISTS public.user_field_schema;

-- from 2026-06-23-000000_native_asset_groups
-- Reverse native asset groups, then restore the directory-membership junction
-- to its original `asset_groups` name.

DROP TABLE IF EXISTS public.asset_group_assignments;

DROP TABLE IF EXISTS public.asset_groups;
DROP SEQUENCE IF EXISTS public.asset_groups_id_seq;

ALTER INDEX idx_asset_directory_memberships_external RENAME TO idx_asset_groups_external;
ALTER INDEX idx_asset_directory_memberships_group RENAME TO idx_asset_groups_group;
ALTER INDEX idx_asset_directory_memberships_asset RENAME TO idx_asset_groups_asset;
ALTER POLICY asset_directory_memberships_workspace_isolation
    ON public.asset_directory_memberships RENAME TO asset_groups_workspace_isolation;
ALTER TABLE public.asset_directory_memberships RENAME TO asset_groups;

-- from 2026-06-21-120000_asset_model_catalog
-- Reverse the asset model catalog.

DROP INDEX IF EXISTS public.idx_assets_model;
ALTER TABLE public.assets DROP CONSTRAINT IF EXISTS assets_model_id_fkey;
ALTER TABLE public.assets DROP COLUMN IF EXISTS model_id;

DROP TABLE IF EXISTS public.asset_models;
DROP TABLE IF EXISTS public.manufacturers;

-- from 2026-06-21-010000_loan_notification_types
DELETE FROM public.notification_types WHERE code IN ('loan_due_soon', 'loan_overdue');
SELECT pg_catalog.setval('public.notification_types_id_seq', 8, true);

-- from 2026-06-21-000000_asset_loans
-- Drops the loan ledger; the sequence, indexes, policy, and triggers go with it.
DROP TABLE IF EXISTS public.asset_loans;

-- from 2026-06-20-030000_tickets_spam_suspected
ALTER TABLE public.tickets DROP COLUMN IF EXISTS spam_suspected;

-- from 2026-06-20-020000_ticket_merges_satellite
-- Re-add the columns to tickets, copy the data back, restore the check, drop
-- the satellite.
ALTER TABLE public.tickets
    ADD COLUMN merged_into_ticket_id integer,
    ADD COLUMN merged_at timestamp with time zone,
    ADD COLUMN merged_by_user_uuid uuid,
    ADD COLUMN merge_reason text;

UPDATE public.tickets t
SET merged_into_ticket_id = m.merged_into_ticket_id,
    merged_at = m.merged_at,
    merged_by_user_uuid = m.merged_by_user_uuid,
    merge_reason = m.merge_reason
FROM public.ticket_merges m
WHERE t.id = m.ticket_id;

ALTER TABLE public.tickets
    ADD CONSTRAINT tickets_merge_complete CHECK (
        ((merged_into_ticket_id IS NULL) AND (merged_at IS NULL) AND (merged_by_user_uuid IS NULL))
        OR ((merged_into_ticket_id IS NOT NULL) AND (merged_at IS NOT NULL) AND (merged_by_user_uuid IS NOT NULL))
    );

DROP TABLE public.ticket_merges;

-- from 2026-06-20-010000_inbound_dead_letters
DROP TABLE IF EXISTS public.inbound_dead_letters;

-- from 2026-06-20-000000_inbound_forwarding
DROP TABLE IF EXISTS public.inbound_addresses;

-- from 2026-06-18-120000_outbound_emails_mail_class
-- Dropping the column removes its check constraint too.
ALTER TABLE public.outbound_emails DROP COLUMN IF EXISTS mail_class;

-- from 2026-06-17-010000_drop_outbound_emails_delivered_at
-- Restore the column as it was in the initial schema: nullable, no default.
ALTER TABLE outbound_emails ADD COLUMN delivered_at timestamp with time zone;

-- from 2026-06-17-000000_workspace_email_settings_dkim
-- Restore the audit trigger to redact only the SMTP password.
DROP TRIGGER tr_audit_workspace_email_settings ON public.workspace_email_settings;
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR UPDATE OR DELETE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password');

ALTER TABLE public.workspace_email_settings
    DROP CONSTRAINT IF EXISTS workspace_email_settings_dkim_algorithm_check,
    DROP CONSTRAINT IF EXISTS workspace_email_settings_verification_status_check,
    DROP CONSTRAINT IF EXISTS workspace_email_settings_sending_mode_check,
    DROP COLUMN IF EXISTS verified_at,
    DROP COLUMN IF EXISTS verification_status,
    DROP COLUMN IF EXISTS dkim_kek_id,
    DROP COLUMN IF EXISTS encrypted_dkim_private_key,
    DROP COLUMN IF EXISTS dkim_algorithm,
    DROP COLUMN IF EXISTS dkim_selector,
    DROP COLUMN IF EXISTS sending_domain,
    DROP COLUMN IF EXISTS sending_mode;

-- from 2026-06-16-130000_outbound_emails_sender_identity
-- Dropping the column removes its check constraint too.
ALTER TABLE public.outbound_emails DROP COLUMN IF EXISTS sender_identity;

-- from 2026-06-16-120000_workspace_email_settings
-- Dropping the table removes its policy, trigger, grants, and constraints.
DROP TABLE IF EXISTS public.workspace_email_settings;

