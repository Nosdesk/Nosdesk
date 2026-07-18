-- Store the OIDC id_token minted at login on the session row, so a later
-- RP-initiated logout can pass it as `id_token_hint` to the provider's
-- end_session endpoint (Hydra rejects a post_logout_redirect_uri without it).
-- Only the OIDC login paths (web callback + mobile native-login) populate it;
-- local/password logins leave it NULL. Cleared implicitly when the session row
-- is revoked on logout.
--
-- Additive + nullable with no backfill, so there is no audit-trigger backfill
-- hazard; active_sessions carries no audit trigger and no workspace_id anyway.
ALTER TABLE active_sessions
    ADD COLUMN oidc_id_token text;
