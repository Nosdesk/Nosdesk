-- Reverse Phase 1: drop the workspace_id columns from every
-- tenant table, then drop the two new tables. The bootstrap
-- workspace row + memberships go with the tables.
--
-- Order matters: drop columns before dropping workspaces /
-- workspace_members because nothing references them (Phase 1
-- doesn't add the FKs — those come in Phase 4). The DROP
-- COLUMN sweep is the inverse of the up.sql ALTER list.

-- Tenant tables
ALTER TABLE tickets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE comments DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE attachments DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE linked_tickets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE ticket_categories DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE ticket_assets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE ticket_tags DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE ticket_watchers DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE tags DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE cycles DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE cycle_tickets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE projects DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE project_tickets DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE workflow_states DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE assignment_rules DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE assignment_rule_state DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE assignment_log DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE sla_policies DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE working_calendars DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE working_calendar_holidays DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE assets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE asset_groups DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE asset_kinds DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE asset_usage_log DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE asset_audits DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE groups DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE user_groups DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE group_includes DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE category_group_visibility DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE documentation_pages DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_collections DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_collection_pages DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_collection_visibility DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_page_visibility DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_page_embeddings DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_page_tickets DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_starred_pages DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_subscriptions DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE documentation_revisions DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE article_contents DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE article_content_revisions DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE knowledge_gaps DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE knowledge_gap_signals DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE channels DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE channel_credentials DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE channel_messages DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE outbound_emails DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE api_tokens DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE webhooks DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE webhook_deliveries DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE plugins DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE plugin_data DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE plugin_activity DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE plugin_collection_rows DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE plugin_collection_schemas DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE sync_actions DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE sync_delta_tokens DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE sync_history DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE notifications DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE notification_preferences DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE audit_log DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE import_jobs DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE backup_jobs DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE csp_reports DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE search_query_log DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE saved_views DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE canned_responses DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE user_ticket_views DROP COLUMN IF EXISTS workspace_id;

ALTER TABLE site_settings DROP COLUMN IF EXISTS workspace_id;

-- New tables (drop order respects FK: workspace_members -> workspaces)
DROP TABLE IF EXISTS workspace_members;
DROP TABLE IF EXISTS workspaces;
