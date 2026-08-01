-- Alarm-discipline #2: whether a notification kind interrupts by default.
-- Drives the default in-app frequency — `true` -> 'instant' (toast + desktop),
-- `false` -> 'quiet' (lands in the bell + badge, no interrupt). A user can still
-- override per (type, channel) via notification_preferences.
ALTER TABLE notification_types ADD COLUMN interrupts BOOLEAN NOT NULL DEFAULT true;

-- Informational kinds default to quiet; alarm / direct-human-comms kinds
-- (mentioned, comment_added, ticket_assigned, sla_breached, loan_overdue) keep
-- the interrupting default.
UPDATE notification_types SET interrupts = false
  WHERE code IN (
    'ticket_status_changed',
    'asset_low_stock',
    'loan_due_soon',
    'doc_page_updated',
    'ticket_created_requester'
  );
