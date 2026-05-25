-- C/W2: make security_events.user_uuid nullable so failed-login and
-- account-lockout events can be recorded for login attempts that
-- don't resolve to a known account.
--
-- The login path (handlers/auth.rs) deliberately makes "unknown
-- email" and "wrong password" indistinguishable in wall-clock time
-- (AUD-007 timing-attack resistance via login_timing::verify_credentials,
-- which returns Option<User> without revealing which case failed). At
-- the failure branch there is therefore no user_uuid to attribute the
-- event to. PCI DSS 10.2.4 ("invalid logical access attempts") and
-- NIST AU-2(3) ("logon failures") want these recorded regardless of
-- whether the account exists.
--
-- NULL is FK-compatible: the security_events_user_uuid_fkey FK to
-- users(uuid) permits NULL (a NULL FK value is simply not checked).
-- Anonymous-attempt rows carry the attempted identifier in `details`
-- instead.

ALTER TABLE security_events ALTER COLUMN user_uuid DROP NOT NULL;
