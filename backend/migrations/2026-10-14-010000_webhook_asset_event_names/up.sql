-- Asset webhook events are named asset.*. The API used to offer device.*,
-- which it accepted but never delivered, so rename any stored device.*
-- subscription to its asset.* equivalent. The rename is deduplicated in case a
-- webhook somehow holds both names.
--
-- webhooks is audited (tr_audit_webhooks) and its trigger needs
-- app.workspace_id, which a migration doesn't set (NDX01), so the trigger is
-- off for the rewrite.
ALTER TABLE public.webhooks DISABLE TRIGGER tr_audit_webhooks;
UPDATE public.webhooks w
SET events = ARRAY(
    SELECT DISTINCT regexp_replace(e, '^device\.', 'asset.')
    FROM unnest(w.events) AS e
    ORDER BY 1
)
WHERE EXISTS (SELECT 1 FROM unnest(w.events) AS e WHERE e LIKE 'device.%');
ALTER TABLE public.webhooks ENABLE TRIGGER tr_audit_webhooks;
