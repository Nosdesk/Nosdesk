-- Postgres does not drop enum values; 'plugin_event' stays, unused once no
-- rule has the trigger.
SELECT 1;
