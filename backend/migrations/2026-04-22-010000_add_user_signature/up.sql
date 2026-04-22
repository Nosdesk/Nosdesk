-- Add per-user email signature, appended to outbound channel replies
-- via `services::channels::outbound::spawn_relay_for_comment`. Free-
-- form text; users own their formatting. Capped at 2000 chars at
-- the handler level (not in schema) to allow future relaxation
-- without a migration.
ALTER TABLE users ADD COLUMN signature TEXT;
