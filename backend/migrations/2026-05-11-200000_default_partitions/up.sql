-- W6b — default partition as outage parachute.
--
-- The partition rotator (services::scheduler + sync::partitions) provisions
-- monthly children with a 60-day lookahead. If the rotator is paused or the
-- pod is down across its window, an INSERT to occurred_at outside the
-- provisioned ranges errors with `no partition of relation "X" found for
-- row` — surfacing as a 500 to the user, who doesn't care that the audit
-- pipeline lapsed.
--
-- A DEFAULT child catches those rows. It's the parachute, not the resting
-- place: when rotation catches up the operator should migrate
-- default-partition rows into the proper child (see partition-recovery
-- runbook). The rotator already logs a WARN if the default has any rows
-- (see services/scheduled_jobs.rs::ensure_partitions); this migration
-- creates the children themselves.
--
-- DEFAULT partitions also block subsequent ATTACH operations if any row
-- would belong to the new partition (Postgres scans the default to validate).
-- For monthly forward-rotation that's fine — new partitions cover future
-- months that have no rows yet — but it's a foot-gun for retroactive
-- backfill. Documented in the runbook.

CREATE TABLE audit_log_default PARTITION OF audit_log DEFAULT;
CREATE TABLE sync_actions_default PARTITION OF sync_actions DEFAULT;
