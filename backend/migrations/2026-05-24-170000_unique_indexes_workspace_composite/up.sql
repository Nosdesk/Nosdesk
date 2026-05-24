-- Phase 3i.2 — composite-workspace UNIQUE INDEXes the Phase 3h.8
-- audit missed.
--
-- Phase 3h.8 checked pg_constraint (named UNIQUE constraints) but
-- not pg_indexes (UNIQUE INDEXes declared via CREATE UNIQUE INDEX).
-- The second-pass external review caught seven distinct functional
-- bugs / cross-tenant existence leaks via the duplicate-key error
-- channel, plus four self-documentation improvements on indexes
-- that are technically safe (FK-scoped) but read like leaks at
-- review time.
--
-- This migration uses DROP INDEX + CREATE UNIQUE INDEX rather than
-- ALTER INDEX because Postgres doesn't support changing a UNIQUE
-- index's column list in place. Each pair is bracketed inside a
-- single transaction (Diesel migrations run atomically) so a
-- failure leaves the table with the old index intact.
--
-- Decision matrix for each touched index:
--
-- HARD BLOCKERS (post-flip + Phase 3i: workspace lifecycle) —
-- second workspace literally cannot exist without these fixes:
--
--   * workflow_states_default_unique
--     - was (is_default) WHERE is_default = TRUE
--     - now (workspace_id, is_default) WHERE is_default = TRUE
--     - every workspace gets its own default workflow state.
--
--   * workflow_states_category_position
--     - was (category, position) WHERE archived_at IS NULL
--     - now (workspace_id, category, position) WHERE archived_at IS NULL
--     - every workspace gets its own per-category ordering.
--
--   * sla_policies_one_default
--     - was (is_default) WHERE is_default = TRUE
--     - now (workspace_id, is_default) WHERE is_default = TRUE
--     - every workspace gets its own default SLA policy.
--
--   * working_calendars_one_default
--     - was (is_default) WHERE is_default = TRUE
--     - now (workspace_id, is_default) WHERE is_default = TRUE
--     - every workspace gets its own default working calendar.
--
-- FUNCTIONAL BUGS + EXISTENCE LEAKS (cross-tenant collision
-- silently corrupts data even at single-tenant scale today; the
-- error message text is the side channel):
--
--   * csp_reports_dedup_hash_idx
--     - was (dedup_hash). dedup_hash is browser-controlled
--       (computed from CSP-violation fields the browser sends),
--       so tenant A's report can collide with tenant B's report
--       and trigger an upsert across tenants — silently merging
--       one tenant's row into the other.
--     - now (workspace_id, dedup_hash).
--
--   * outbound_emails_idempotency_key_uidx
--     - was (idempotency_key) WHERE idempotency_key IS NOT NULL.
--       idempotency_key is currently generated as a SHA-like
--       hash from message contents; two tenants legitimately
--       sending similar emails could collide, and the duplicate-
--       key error cancels the second tenant's send.
--     - now (workspace_id, idempotency_key) WHERE idempotency_key IS NOT NULL.
--
--   * sync_delta_tokens_provider_type_entity_type_key
--     - was (provider_type, entity_type). The Microsoft Graph
--       delta-sync state is per-tenant (each workspace connects
--       its own Azure AD tenant), so the existing constraint
--       allows only the FIRST workspace to sync and silently
--       rejects every other workspace's delta-token write.
--     - now (workspace_id, provider_type, entity_type).
--
-- SELF-DOCUMENTATION (FK-scoped already so no actual leak, but
-- a future reviewer doing "grep UNIQUE on tenant tables, where's
-- workspace_id?" will flag these as suspicious — composite makes
-- the intent explicit):
--
--   * idx_collection_vis_group
--   * idx_collection_vis_user
--   * idx_page_vis_group
--   * idx_page_vis_user
--
-- NOT TOUCHED (intent-questionable, separate product decisions):
--
--   * idx_asset_serial_unique on assets(serial_number) WHERE
--     serial_number IS NOT NULL. Real-world device serial
--     numbers are globally unique by manufacturer, so a global
--     UNIQUE is defensible. If two tenants legitimately want to
--     register the same imported device, this becomes a bug.
--     Flag for product decision separately; not changed here.
--
--   * idx_groups_external_id on groups(external_id) WHERE
--     external_id IS NOT NULL. Each workspace connects its own
--     Azure AD tenant; group external_ids might be globally
--     unique (Azure object IDs are GUIDs) or per-tenant. The
--     intent is unclear without product context. Not changed.
--
--   * notification_preferences_user_uuid_notification_type_id_cha_key
--     — already flagged as P2 in the 3h.8 audit comment. Cross-
--     workspace prefs are a correctness concern, not an
--     existence leak. Phase 3i+ work.

-- ---------- Phase 3i workspace-lifecycle blockers ----------

DROP INDEX IF EXISTS workflow_states_default_unique;
CREATE UNIQUE INDEX workflow_states_default_unique
    ON workflow_states (workspace_id, is_default)
    WHERE is_default = true;

DROP INDEX IF EXISTS workflow_states_category_position;
CREATE UNIQUE INDEX workflow_states_category_position
    ON workflow_states (workspace_id, category, position)
    WHERE archived_at IS NULL;

DROP INDEX IF EXISTS sla_policies_one_default;
CREATE UNIQUE INDEX sla_policies_one_default
    ON sla_policies (workspace_id, is_default)
    WHERE is_default = true;

DROP INDEX IF EXISTS working_calendars_one_default;
CREATE UNIQUE INDEX working_calendars_one_default
    ON working_calendars (workspace_id, is_default)
    WHERE is_default = true;

-- ---------- Functional bugs / existence leaks ----------

DROP INDEX IF EXISTS csp_reports_dedup_hash_idx;
CREATE UNIQUE INDEX csp_reports_dedup_hash_idx
    ON csp_reports (workspace_id, dedup_hash);

DROP INDEX IF EXISTS outbound_emails_idempotency_key_uidx;
CREATE UNIQUE INDEX outbound_emails_idempotency_key_uidx
    ON outbound_emails (workspace_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

ALTER TABLE sync_delta_tokens
    DROP CONSTRAINT IF EXISTS sync_delta_tokens_provider_type_entity_type_key;
ALTER TABLE sync_delta_tokens
    ADD CONSTRAINT sync_delta_tokens_workspace_provider_entity_key
    UNIQUE (workspace_id, provider_type, entity_type);

-- ---------- Self-documentation (FK-scoped but read suspicious) ----------

DROP INDEX IF EXISTS idx_collection_vis_group;
CREATE UNIQUE INDEX idx_collection_vis_group
    ON documentation_collection_visibility (workspace_id, collection_id, group_id)
    WHERE group_id IS NOT NULL;

DROP INDEX IF EXISTS idx_collection_vis_user;
CREATE UNIQUE INDEX idx_collection_vis_user
    ON documentation_collection_visibility (workspace_id, collection_id, user_uuid)
    WHERE user_uuid IS NOT NULL;

DROP INDEX IF EXISTS idx_page_vis_group;
CREATE UNIQUE INDEX idx_page_vis_group
    ON documentation_page_visibility (workspace_id, page_id, group_id)
    WHERE group_id IS NOT NULL;

DROP INDEX IF EXISTS idx_page_vis_user;
CREATE UNIQUE INDEX idx_page_vis_user
    ON documentation_page_visibility (workspace_id, page_id, user_uuid)
    WHERE user_uuid IS NOT NULL;
