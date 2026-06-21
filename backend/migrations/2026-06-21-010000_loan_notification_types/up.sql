-- Notification types for the device-loan due-back reminders (Phase 3).
--
-- `notification_types` is a non-tenant system catalog (no workspace_id, no
-- RLS), so this is a plain seed. The scheduler's loan-reminder job dispatches
-- these to the borrower via NotificationService (in-app + email), per the
-- default_channels here and each recipient's preferences.
INSERT INTO public.notification_types (id, code, name, description, category, default_channels) VALUES
  (9, 'loan_due_soon', 'Loan Due Soon', 'When a device you have on loan is due back soon', 'asset', '["in_app", "email"]'),
  (10, 'loan_overdue', 'Loan Overdue', 'When a device you have on loan is overdue', 'asset', '["in_app", "email"]');

SELECT pg_catalog.setval('public.notification_types_id_seq', 10, true);
