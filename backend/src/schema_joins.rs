//! `joinable!` for the foreign keys between workspace tables.
//!
//! Those keys are two-column, `(workspace_id, x_id)` referencing the parent's
//! `(workspace_id, id)`, and `diesel print-schema` emits `joinable!` only for
//! single-column keys, so `make schema` leaves them out of `schema.rs`. Each
//! line here declares the join on the non-workspace column, the one queries
//! join on. Add a line when a migration adds such a key and code joins across
//! it.

use crate::schema::*;

diesel::joinable!(article_content_revisions -> article_contents (article_content_id));
diesel::joinable!(article_contents -> tickets (ticket_id));
diesel::joinable!(asset_audits -> assets (asset_id));
diesel::joinable!(asset_directory_memberships -> assets (asset_id));
diesel::joinable!(asset_directory_memberships -> groups (group_id));
diesel::joinable!(asset_disposals -> asset_lifecycle_events (lifecycle_event_id));
diesel::joinable!(asset_disposals -> assets (asset_id));
diesel::joinable!(asset_group_assignments -> asset_groups (group_id));
diesel::joinable!(asset_group_assignments -> assets (asset_id));
diesel::joinable!(asset_lifecycle_events -> assets (asset_id));
diesel::joinable!(asset_lifecycle_events -> tickets (ticket_id));
diesel::joinable!(asset_loans -> assets (asset_id));
diesel::joinable!(asset_loans -> tickets (ticket_id));
diesel::joinable!(asset_media -> assets (asset_id));
diesel::joinable!(asset_models -> manufacturers (manufacturer_id));
diesel::joinable!(asset_usage_log -> assets (asset_id));
diesel::joinable!(asset_usage_log -> tickets (ticket_id));
diesel::joinable!(assets -> asset_models (model_id));
diesel::joinable!(assignment_log -> assignment_rules (rule_id));
diesel::joinable!(assignment_log -> tickets (ticket_id));
diesel::joinable!(assignment_rule_state -> assignment_rules (rule_id));
diesel::joinable!(assignment_rules -> groups (target_group_id));
diesel::joinable!(assignment_rules -> ticket_categories (category_id));
diesel::joinable!(attachments -> comments (comment_id));
diesel::joinable!(canned_response_insertions -> canned_responses (canned_response_id));
diesel::joinable!(canned_response_insertions -> tickets (ticket_id));
diesel::joinable!(category_approvers -> ticket_categories (category_id));
diesel::joinable!(category_group_visibility -> groups (group_id));
diesel::joinable!(category_group_visibility -> ticket_categories (category_id));
diesel::joinable!(channel_credentials -> channels (channel_id));
diesel::joinable!(channel_messages -> channels (channel_id));
diesel::joinable!(channel_messages -> comments (comment_id));
diesel::joinable!(channel_messages -> tickets (ticket_id));
diesel::joinable!(comment_ticket_references -> comments (comment_id));
diesel::joinable!(comment_ticket_references -> tickets (referenced_ticket_id));
diesel::joinable!(comments -> tickets (ticket_id));
diesel::joinable!(cycle_tickets -> cycles (cycle_id));
diesel::joinable!(cycle_tickets -> tickets (ticket_id));
diesel::joinable!(cycles -> projects (project_id));
diesel::joinable!(documentation_collection_pages -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_collection_visibility -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_visibility -> groups (group_id));
diesel::joinable!(documentation_page_tickets -> documentation_pages (page_id));
diesel::joinable!(documentation_page_tickets -> tickets (ticket_id));
diesel::joinable!(documentation_page_visibility -> documentation_pages (page_id));
diesel::joinable!(documentation_page_visibility -> groups (group_id));
diesel::joinable!(documentation_revisions -> documentation_pages (page_id));
diesel::joinable!(documentation_starred_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_subscriptions -> documentation_pages (page_id));
diesel::joinable!(inbound_addresses -> channels (channel_id));
diesel::joinable!(knowledge_gap_signals -> knowledge_gaps (gap_id));
diesel::joinable!(notification_deliveries -> notifications (notification_id));
diesel::joinable!(outbound_emails -> channels (channel_id));
diesel::joinable!(outbound_emails -> comments (comment_id));
diesel::joinable!(outbound_emails -> tickets (ticket_id));
diesel::joinable!(plugin_activity -> plugins (plugin_id));
diesel::joinable!(plugin_collection_rows -> plugin_collection_schemas (schema_id));
diesel::joinable!(plugin_collection_rows -> plugins (plugin_id));
diesel::joinable!(plugin_collection_schemas -> plugins (plugin_id));
diesel::joinable!(plugin_data -> plugins (plugin_id));
diesel::joinable!(project_tickets -> projects (project_id));
diesel::joinable!(project_tickets -> tickets (ticket_id));
diesel::joinable!(rule_applications -> rules (rule_id));
diesel::joinable!(rule_applications -> tickets (ticket_id));
diesel::joinable!(rule_versions -> rules (rule_id));
diesel::joinable!(sla_policies -> groups (assignee_group_id_filter));
diesel::joinable!(sla_policies -> ticket_categories (category_id_filter));
diesel::joinable!(sla_policies -> working_calendars (working_calendar_id));
diesel::joinable!(ticket_approvals -> tickets (ticket_id));
diesel::joinable!(ticket_assets -> assets (asset_id));
diesel::joinable!(ticket_assets -> tickets (ticket_id));
diesel::joinable!(ticket_ratings -> tickets (ticket_id));
diesel::joinable!(ticket_tags -> tags (tag_id));
diesel::joinable!(ticket_tags -> tickets (ticket_id));
diesel::joinable!(ticket_watchers -> tickets (ticket_id));
diesel::joinable!(tickets -> channels (origin_channel_id));
diesel::joinable!(tickets -> ticket_categories (category_id));
diesel::joinable!(tickets -> workflow_states (workflow_state_id));
diesel::joinable!(user_groups -> groups (group_id));
diesel::joinable!(user_ticket_views -> tickets (ticket_id));
diesel::joinable!(webhook_deliveries -> webhooks (webhook_id));
diesel::joinable!(working_calendar_holidays -> working_calendars (calendar_id));
diesel::joinable!(workspace_notices -> tickets (incident_ticket_id));
