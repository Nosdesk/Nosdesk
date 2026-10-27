-- When this session last proved who is using it: a password, LDAP, passkey or
-- authenticator code sign-in, a provider sign-in that asked for credentials
-- (the id_token's auth_time), or a confirmation in a step-up dialog. Actions
-- that need a step-up go ahead only within a short window after it. NULL when
-- the session has proved nothing yet (a remembered provider sign-in without
-- auth_time, a portal session, a legacy refresh).
--
-- Additive + nullable with no backfill: existing sessions confirm again before
-- their next step-up action. active_sessions carries no audit trigger.
ALTER TABLE active_sessions
    ADD COLUMN confirmed_at timestamptz;
