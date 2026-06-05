-- User as a sync aggregate. Adding the variant lets the directory
-- composable on the frontend collapse onto `useReference('user', uuid)`
-- so assignee / requester avatars render on first paint without an
-- extra `/users/batch` round trip after the ticket cards land.
--
-- ALTER TYPE ... ADD VALUE inside a transaction is fine on Postgres
-- 12+; the new value just can't be used in the same transaction.
-- compose.yaml pins postgres:17.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'user';
