-- Notification as a sync aggregate. Lets notifications ride the
-- sync_actions change-stream (delivered to every backend machine via
-- Postgres LISTEN/NOTIFY) instead of the in-process broadcast that was
-- instance-local. Rows are scoped to the recipient's private
-- `user:<uuid>` group so only that user's clients see them.
--
-- ALTER TYPE ... ADD VALUE inside a transaction is fine on Postgres
-- 12+; the new value just can't be used in the same transaction.
-- compose.yaml pins postgres:17.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'notification';
