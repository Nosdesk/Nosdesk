-- Reverse the composite-workspace UNIQUE INDEX migration.
-- Down only succeeds if no cross-workspace duplicate values exist
-- on the columns being re-narrowed (which would now be allowed
-- under the composite shape but rejected by the global shape).

DROP INDEX IF EXISTS idx_page_vis_user;
CREATE UNIQUE INDEX idx_page_vis_user
    ON documentation_page_visibility (page_id, user_uuid)
    WHERE user_uuid IS NOT NULL;

DROP INDEX IF EXISTS idx_page_vis_group;
CREATE UNIQUE INDEX idx_page_vis_group
    ON documentation_page_visibility (page_id, group_id)
    WHERE group_id IS NOT NULL;

DROP INDEX IF EXISTS idx_collection_vis_user;
CREATE UNIQUE INDEX idx_collection_vis_user
    ON documentation_collection_visibility (collection_id, user_uuid)
    WHERE user_uuid IS NOT NULL;

DROP INDEX IF EXISTS idx_collection_vis_group;
CREATE UNIQUE INDEX idx_collection_vis_group
    ON documentation_collection_visibility (collection_id, group_id)
    WHERE group_id IS NOT NULL;

ALTER TABLE sync_delta_tokens
    DROP CONSTRAINT IF EXISTS sync_delta_tokens_workspace_provider_entity_key;
ALTER TABLE sync_delta_tokens
    ADD CONSTRAINT sync_delta_tokens_provider_type_entity_type_key
    UNIQUE (provider_type, entity_type);

DROP INDEX IF EXISTS outbound_emails_idempotency_key_uidx;
CREATE UNIQUE INDEX outbound_emails_idempotency_key_uidx
    ON outbound_emails (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

DROP INDEX IF EXISTS csp_reports_dedup_hash_idx;
CREATE UNIQUE INDEX csp_reports_dedup_hash_idx
    ON csp_reports (dedup_hash);

DROP INDEX IF EXISTS working_calendars_one_default;
CREATE UNIQUE INDEX working_calendars_one_default
    ON working_calendars (is_default)
    WHERE is_default = true;

DROP INDEX IF EXISTS sla_policies_one_default;
CREATE UNIQUE INDEX sla_policies_one_default
    ON sla_policies (is_default)
    WHERE is_default = true;

DROP INDEX IF EXISTS workflow_states_category_position;
CREATE UNIQUE INDEX workflow_states_category_position
    ON workflow_states (category, position)
    WHERE archived_at IS NULL;

DROP INDEX IF EXISTS workflow_states_default_unique;
CREATE UNIQUE INDEX workflow_states_default_unique
    ON workflow_states (is_default)
    WHERE is_default = true;
