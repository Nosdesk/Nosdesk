-- Regression fix: remove the W4 (2026-05-25-140000) audit_log triggers
-- from the auth / identity tables.
--
-- audit_log.workspace_id is NOT NULL and defaults from the
-- app.workspace_id GUC. These tables are platform-level (no
-- workspace_id of their own) and are written in flows that run BEFORE
-- any workspace context is established: session creation on login
-- (active_sessions), token issuance / rotation (refresh_tokens,
-- api_tokens), password-reset requests (reset_tokens), passkey and
-- OAuth login (passkey_credentials, user_auth_identities), and
-- onboarding / invitation acceptance (user_emails). With no
-- app.workspace_id set, the trigger's audit_log insert defaults
-- workspace_id to NULL and fails the NOT NULL constraint, which 500s
-- the whole operation. The net effect since W4: every login and token
-- refresh has been failing at the "create session" step.
--
-- These tables' security-relevant events already flow to the tier-2
-- security_events table (login attempts, MFA changes, password resets,
-- token lifecycle), which is the correct platform-level audit
-- substrate and is intentionally NOT workspace-scoped. The pre-W4
-- design deliberately excluded them from tier-3 for exactly this
-- reason; W4 reversed that without accounting for the workspace-context
-- requirement.

DROP TRIGGER IF EXISTS tr_audit_active_sessions ON active_sessions;
DROP TRIGGER IF EXISTS tr_audit_api_tokens ON api_tokens;
DROP TRIGGER IF EXISTS tr_audit_refresh_tokens ON refresh_tokens;
DROP TRIGGER IF EXISTS tr_audit_reset_tokens ON reset_tokens;
DROP TRIGGER IF EXISTS tr_audit_passkey_credentials ON passkey_credentials;
DROP TRIGGER IF EXISTS tr_audit_user_emails ON user_emails;
DROP TRIGGER IF EXISTS tr_audit_user_auth_identities ON user_auth_identities;
