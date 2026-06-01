-- =====================================================================
-- Add `is_platform_scoped` flag to api_tokens.
--
-- Platform-scoped tokens are minted operator-side (control plane) and
-- used for internal provisioning calls back into the product
-- (workspace create, eager owner projection, custom-domain updates).
-- Unlike user-bound tokens, they're cross-workspace by design — handlers
-- gate on `require_platform_scope()` and run their writes through
-- `with_actor_bypass_context` so the BYPASSRLS elevation is auditable
-- via the existing actor-context machinery.
--
-- See `docs/m5-product-side-handoff.md` Task 1 + `account-billing-
-- architecture.md` D8.6 for the design rationale; D4 for the trust-
-- channel model.
--
-- Additive change, NOT NULL DEFAULT false — every existing token rows
-- as user-bound. New tokens default to user-bound; the operator CLI
-- that mints platform tokens passes the flag explicitly.
-- =====================================================================

ALTER TABLE api_tokens
    ADD COLUMN is_platform_scoped BOOLEAN NOT NULL DEFAULT false;

-- Partial index: most tokens are user-bound. Platform-scoped tokens
-- are a small handful (one per operator-cli mint, single-digits
-- across the whole instance) so a partial index keeps lookups for
-- the "list platform tokens" admin view cheap without bloating the
-- index for the common case.
CREATE INDEX api_tokens_platform_scoped_idx
    ON api_tokens (id)
    WHERE is_platform_scoped = true;
