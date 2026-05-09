-- Ticket watchers (a.k.a. followers, subscribers).
--
-- Lets a user opt into notifications for a ticket without being
-- the requester or assignee. Without this, agents typically CC
-- themselves on every comment to fake the notification flow,
-- which clutters the conversation. Watchers separate "I want to
-- be told" from "I'm responsible".
--
-- Composite primary key prevents double-watching; CASCADE on
-- both sides cleans up when the parent disappears. Indexed in
-- both directions because both lookups are hot:
--   - watchers for a ticket (notification fan-out, sidebar list)
--   - tickets watched by a user (future "watching" tab)

CREATE TABLE ticket_watchers (
    ticket_id   INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_uuid   UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    -- When watcher status was assigned. Distinct from the more
    -- precise sync_actions audit trail so the watch UI can sort
    -- by "since" without joining sync_actions.
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Was this watch added implicitly (auto-watch on comment),
    -- or did the user explicitly toggle the bell? Drives a
    -- future "stop auto-watching" preference + the audit trail.
    auto_added  BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (ticket_id, user_uuid)
);

-- Reverse lookup: tickets a given user watches. Drives the
-- (future) "Watching" smart view.
CREATE INDEX ticket_watchers_user_idx ON ticket_watchers (user_uuid);
