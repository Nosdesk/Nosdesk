DELETE FROM public.notification_types WHERE code IN ('loan_due_soon', 'loan_overdue');
SELECT pg_catalog.setval('public.notification_types_id_seq', 8, true);
