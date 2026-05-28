-- Phase 2.1: register the SLA-breach notification kind so the
-- periodic breach-detection sweep (services::scheduled_jobs::
-- detect_sla_breaches) can drop a row in the assignee + watchers'
-- notification feed when a ticket's response or resolution timer
-- breaches.
--
-- Channels: in_app (the bell drop) + email — breaches are the kind
-- of operational event people miss in the bell after-hours but catch
-- in their inbox, and admins/agents can opt out per-channel via the
-- notification preferences UI.

INSERT INTO notification_types (code, name, description, category, default_channels)
VALUES (
    'sla_breached',
    'SLA Breached',
    'When a ticket''s response or resolution SLA target has been missed',
    'ticket',
    '["in_app", "email"]'
);
