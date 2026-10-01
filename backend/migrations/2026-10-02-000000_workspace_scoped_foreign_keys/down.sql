-- Restore the single-column foreign keys and drop the (workspace_id, id)
-- indexes. The cleanup isn't reversed: the rows it changed referenced another
-- workspace's rows.
ALTER TABLE public.article_content_revisions
    DROP CONSTRAINT article_content_revisions_article_content_id_fkey,
    ADD CONSTRAINT article_content_revisions_article_content_id_fkey FOREIGN KEY (article_content_id) REFERENCES public.article_contents (id) ON DELETE CASCADE;
ALTER TABLE public.article_contents
    DROP CONSTRAINT article_contents_ticket_id_fkey,
    ADD CONSTRAINT article_contents_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_audits
    DROP CONSTRAINT asset_audits_asset_id_fkey,
    ADD CONSTRAINT asset_audits_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_directory_memberships
    DROP CONSTRAINT device_groups_device_id_fkey,
    ADD CONSTRAINT device_groups_device_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_directory_memberships
    DROP CONSTRAINT device_groups_group_id_fkey,
    ADD CONSTRAINT device_groups_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.asset_disposals
    DROP CONSTRAINT asset_disposals_asset_id_fkey,
    ADD CONSTRAINT asset_disposals_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_disposals
    DROP CONSTRAINT asset_disposals_lifecycle_event_id_fkey,
    ADD CONSTRAINT asset_disposals_lifecycle_event_id_fkey FOREIGN KEY (lifecycle_event_id) REFERENCES public.asset_lifecycle_events (id) ON DELETE SET NULL;
ALTER TABLE public.asset_group_assignments
    DROP CONSTRAINT asset_group_assignments_asset_id_fkey,
    ADD CONSTRAINT asset_group_assignments_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_group_assignments
    DROP CONSTRAINT asset_group_assignments_group_id_fkey,
    ADD CONSTRAINT asset_group_assignments_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.asset_groups (id) ON DELETE CASCADE;
ALTER TABLE public.asset_lifecycle_events
    DROP CONSTRAINT asset_lifecycle_events_asset_id_fkey,
    ADD CONSTRAINT asset_lifecycle_events_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_lifecycle_events
    DROP CONSTRAINT asset_lifecycle_events_ticket_id_fkey,
    ADD CONSTRAINT asset_lifecycle_events_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_asset_id_fkey,
    ADD CONSTRAINT asset_loans_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_ticket_id_fkey,
    ADD CONSTRAINT asset_loans_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.asset_media
    DROP CONSTRAINT asset_media_asset_id_fkey,
    ADD CONSTRAINT asset_media_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_models
    DROP CONSTRAINT asset_models_manufacturer_id_fkey,
    ADD CONSTRAINT asset_models_manufacturer_id_fkey FOREIGN KEY (manufacturer_id) REFERENCES public.manufacturers (id) ON DELETE RESTRICT;
ALTER TABLE public.asset_usage_log
    DROP CONSTRAINT asset_usage_log_asset_id_fkey,
    ADD CONSTRAINT asset_usage_log_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.asset_usage_log
    DROP CONSTRAINT asset_usage_log_ticket_id_fkey,
    ADD CONSTRAINT asset_usage_log_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.assets
    DROP CONSTRAINT assets_model_id_fkey,
    ADD CONSTRAINT assets_model_id_fkey FOREIGN KEY (model_id) REFERENCES public.asset_models (id) ON DELETE SET NULL;
ALTER TABLE public.assignment_log
    DROP CONSTRAINT assignment_log_rule_id_fkey,
    ADD CONSTRAINT assignment_log_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.assignment_rules (id) ON DELETE SET NULL;
ALTER TABLE public.assignment_log
    DROP CONSTRAINT assignment_log_ticket_id_fkey,
    ADD CONSTRAINT assignment_log_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.assignment_rule_state
    DROP CONSTRAINT assignment_rule_state_rule_id_fkey,
    ADD CONSTRAINT assignment_rule_state_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.assignment_rules (id) ON DELETE CASCADE;
ALTER TABLE public.assignment_rules
    DROP CONSTRAINT assignment_rules_category_id_fkey,
    ADD CONSTRAINT assignment_rules_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories (id) ON DELETE SET NULL;
ALTER TABLE public.assignment_rules
    DROP CONSTRAINT assignment_rules_target_group_id_fkey,
    ADD CONSTRAINT assignment_rules_target_group_id_fkey FOREIGN KEY (target_group_id) REFERENCES public.groups (id) ON DELETE SET NULL;
ALTER TABLE public.attachments
    DROP CONSTRAINT attachments_comment_id_fkey,
    ADD CONSTRAINT attachments_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments (id) ON DELETE CASCADE;
ALTER TABLE public.canned_response_insertions
    DROP CONSTRAINT canned_response_insertions_canned_response_id_fkey,
    ADD CONSTRAINT canned_response_insertions_canned_response_id_fkey FOREIGN KEY (canned_response_id) REFERENCES public.canned_responses (id) ON DELETE CASCADE;
ALTER TABLE public.canned_response_insertions
    DROP CONSTRAINT canned_response_insertions_ticket_id_fkey,
    ADD CONSTRAINT canned_response_insertions_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.category_approvers
    DROP CONSTRAINT category_approvers_category_id_fkey,
    ADD CONSTRAINT category_approvers_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories (id) ON DELETE CASCADE;
ALTER TABLE public.category_group_visibility
    DROP CONSTRAINT category_group_visibility_category_id_fkey,
    ADD CONSTRAINT category_group_visibility_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories (id) ON DELETE CASCADE;
ALTER TABLE public.category_group_visibility
    DROP CONSTRAINT category_group_visibility_group_id_fkey,
    ADD CONSTRAINT category_group_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.channel_credentials
    DROP CONSTRAINT channel_credentials_channel_id_fkey,
    ADD CONSTRAINT channel_credentials_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels (id) ON DELETE CASCADE;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_channel_id_fkey,
    ADD CONSTRAINT channel_messages_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels (id) ON DELETE CASCADE;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_comment_id_fkey,
    ADD CONSTRAINT channel_messages_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments (id) ON DELETE SET NULL;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_ticket_id_fkey,
    ADD CONSTRAINT channel_messages_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.comment_ticket_references
    DROP CONSTRAINT comment_ticket_references_comment_id_fkey,
    ADD CONSTRAINT comment_ticket_references_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments (id) ON DELETE CASCADE;
ALTER TABLE public.comment_ticket_references
    DROP CONSTRAINT comment_ticket_references_referenced_ticket_id_fkey,
    ADD CONSTRAINT comment_ticket_references_referenced_ticket_id_fkey FOREIGN KEY (referenced_ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.comments
    DROP CONSTRAINT comments_ticket_id_fkey,
    ADD CONSTRAINT comments_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.cycle_tickets
    DROP CONSTRAINT cycle_tickets_cycle_id_fkey,
    ADD CONSTRAINT cycle_tickets_cycle_id_fkey FOREIGN KEY (cycle_id) REFERENCES public.cycles (id) ON DELETE CASCADE;
ALTER TABLE public.cycle_tickets
    DROP CONSTRAINT cycle_tickets_ticket_id_fkey,
    ADD CONSTRAINT cycle_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.cycles
    DROP CONSTRAINT cycles_project_id_fkey,
    ADD CONSTRAINT cycles_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_collection_pages
    DROP CONSTRAINT documentation_collection_pages_collection_id_fkey,
    ADD CONSTRAINT documentation_collection_pages_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.documentation_collections (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_collection_pages
    DROP CONSTRAINT documentation_collection_pages_page_id_fkey,
    ADD CONSTRAINT documentation_collection_pages_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_collection_id_fkey,
    ADD CONSTRAINT documentation_collection_visibility_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.documentation_collections (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_group_id_fkey,
    ADD CONSTRAINT documentation_collection_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_embeddings
    DROP CONSTRAINT documentation_page_embeddings_source_page_id_fkey,
    ADD CONSTRAINT documentation_page_embeddings_source_page_id_fkey FOREIGN KEY (source_page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_embeddings
    DROP CONSTRAINT documentation_page_embeddings_target_page_id_fkey,
    ADD CONSTRAINT documentation_page_embeddings_target_page_id_fkey FOREIGN KEY (target_page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_tickets
    DROP CONSTRAINT documentation_page_tickets_page_id_fkey,
    ADD CONSTRAINT documentation_page_tickets_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_tickets
    DROP CONSTRAINT documentation_page_tickets_ticket_id_fkey,
    ADD CONSTRAINT documentation_page_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_group_id_fkey,
    ADD CONSTRAINT documentation_page_visibility_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_page_id_fkey,
    ADD CONSTRAINT documentation_page_visibility_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_pages
    DROP CONSTRAINT documentation_pages_parent_id_fkey,
    ADD CONSTRAINT documentation_pages_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_revisions
    DROP CONSTRAINT documentation_revisions_page_id_fkey,
    ADD CONSTRAINT documentation_revisions_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_starred_pages
    DROP CONSTRAINT documentation_starred_pages_page_id_fkey,
    ADD CONSTRAINT documentation_starred_pages_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.documentation_subscriptions
    DROP CONSTRAINT documentation_subscriptions_page_id_fkey,
    ADD CONSTRAINT documentation_subscriptions_page_id_fkey FOREIGN KEY (page_id) REFERENCES public.documentation_pages (id) ON DELETE CASCADE;
ALTER TABLE public.group_includes
    DROP CONSTRAINT group_includes_child_group_id_fkey,
    ADD CONSTRAINT group_includes_child_group_id_fkey FOREIGN KEY (child_group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.group_includes
    DROP CONSTRAINT group_includes_parent_group_id_fkey,
    ADD CONSTRAINT group_includes_parent_group_id_fkey FOREIGN KEY (parent_group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.inbound_addresses
    DROP CONSTRAINT inbound_addresses_channel_id_fkey,
    ADD CONSTRAINT inbound_addresses_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels (id) ON DELETE CASCADE;
ALTER TABLE public.knowledge_gap_signals
    DROP CONSTRAINT knowledge_gap_signals_gap_id_fkey,
    ADD CONSTRAINT knowledge_gap_signals_gap_id_fkey FOREIGN KEY (gap_id) REFERENCES public.knowledge_gaps (id) ON DELETE CASCADE;
ALTER TABLE public.knowledge_gaps
    DROP CONSTRAINT knowledge_gaps_draft_page_id_fkey,
    ADD CONSTRAINT knowledge_gaps_draft_page_id_fkey FOREIGN KEY (draft_page_id) REFERENCES public.documentation_pages (id) ON DELETE SET NULL;
ALTER TABLE public.knowledge_gaps
    DROP CONSTRAINT knowledge_gaps_resolved_page_id_fkey,
    ADD CONSTRAINT knowledge_gaps_resolved_page_id_fkey FOREIGN KEY (resolved_page_id) REFERENCES public.documentation_pages (id) ON DELETE SET NULL;
ALTER TABLE public.linked_tickets
    DROP CONSTRAINT linked_tickets_linked_ticket_id_fkey,
    ADD CONSTRAINT linked_tickets_linked_ticket_id_fkey FOREIGN KEY (linked_ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.linked_tickets
    DROP CONSTRAINT linked_tickets_ticket_id_fkey,
    ADD CONSTRAINT linked_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.notification_deliveries
    DROP CONSTRAINT notification_deliveries_notification_id_fkey,
    ADD CONSTRAINT notification_deliveries_notification_id_fkey FOREIGN KEY (notification_id) REFERENCES public.notifications (id) ON DELETE CASCADE;
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_channel_id_fkey,
    ADD CONSTRAINT outbound_emails_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels (id);
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_comment_id_fkey,
    ADD CONSTRAINT outbound_emails_comment_id_fkey FOREIGN KEY (comment_id) REFERENCES public.comments (id) ON DELETE SET NULL;
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_ticket_id_fkey,
    ADD CONSTRAINT outbound_emails_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.plugin_activity
    DROP CONSTRAINT plugin_activity_plugin_id_fkey,
    ADD CONSTRAINT plugin_activity_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins (id) ON DELETE CASCADE;
ALTER TABLE public.plugin_collection_rows
    DROP CONSTRAINT plugin_collection_rows_plugin_id_fkey,
    ADD CONSTRAINT plugin_collection_rows_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins (id) ON DELETE CASCADE;
ALTER TABLE public.plugin_collection_rows
    DROP CONSTRAINT plugin_collection_rows_schema_id_fkey,
    ADD CONSTRAINT plugin_collection_rows_schema_id_fkey FOREIGN KEY (schema_id) REFERENCES public.plugin_collection_schemas (id) ON DELETE CASCADE;
ALTER TABLE public.plugin_collection_schemas
    DROP CONSTRAINT plugin_collection_schemas_plugin_id_fkey,
    ADD CONSTRAINT plugin_collection_schemas_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins (id) ON DELETE CASCADE;
ALTER TABLE public.plugin_data
    DROP CONSTRAINT plugin_data_plugin_id_fkey,
    ADD CONSTRAINT plugin_data_plugin_id_fkey FOREIGN KEY (plugin_id) REFERENCES public.plugins (id) ON DELETE CASCADE;
ALTER TABLE public.project_tickets
    DROP CONSTRAINT project_tickets_project_id_fkey,
    ADD CONSTRAINT project_tickets_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects (id) ON DELETE CASCADE;
ALTER TABLE public.project_tickets
    DROP CONSTRAINT project_tickets_ticket_id_fkey,
    ADD CONSTRAINT project_tickets_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.rule_applications
    DROP CONSTRAINT rule_applications_rule_id_fkey,
    ADD CONSTRAINT rule_applications_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.rules (id) ON DELETE CASCADE;
ALTER TABLE public.rule_applications
    DROP CONSTRAINT rule_applications_ticket_id_fkey,
    ADD CONSTRAINT rule_applications_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.rule_versions
    DROP CONSTRAINT rule_versions_rule_id_fkey,
    ADD CONSTRAINT rule_versions_rule_id_fkey FOREIGN KEY (rule_id) REFERENCES public.rules (id) ON DELETE CASCADE;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_assignee_group_id_filter_fkey,
    ADD CONSTRAINT sla_policies_assignee_group_id_filter_fkey FOREIGN KEY (assignee_group_id_filter) REFERENCES public.groups (id) ON DELETE SET NULL;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_category_id_filter_fkey,
    ADD CONSTRAINT sla_policies_category_id_filter_fkey FOREIGN KEY (category_id_filter) REFERENCES public.ticket_categories (id) ON DELETE SET NULL;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_working_calendar_id_fkey,
    ADD CONSTRAINT sla_policies_working_calendar_id_fkey FOREIGN KEY (working_calendar_id) REFERENCES public.working_calendars (id) ON DELETE SET NULL;
ALTER TABLE public.ticket_approvals
    DROP CONSTRAINT ticket_approvals_ticket_id_fkey,
    ADD CONSTRAINT ticket_approvals_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_assets
    DROP CONSTRAINT ticket_devices_device_id_fkey,
    ADD CONSTRAINT ticket_devices_device_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_assets
    DROP CONSTRAINT ticket_devices_ticket_id_fkey,
    ADD CONSTRAINT ticket_devices_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_merges
    DROP CONSTRAINT ticket_merges_merged_into_ticket_id_fkey,
    ADD CONSTRAINT ticket_merges_merged_into_ticket_id_fkey FOREIGN KEY (merged_into_ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_merges
    DROP CONSTRAINT ticket_merges_ticket_id_fkey,
    ADD CONSTRAINT ticket_merges_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_ratings
    DROP CONSTRAINT ticket_ratings_ticket_id_fkey,
    ADD CONSTRAINT ticket_ratings_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_tags
    DROP CONSTRAINT ticket_tags_tag_id_fkey,
    ADD CONSTRAINT ticket_tags_tag_id_fkey FOREIGN KEY (tag_id) REFERENCES public.tags (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_tags
    DROP CONSTRAINT ticket_tags_ticket_id_fkey,
    ADD CONSTRAINT ticket_tags_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.ticket_watchers
    DROP CONSTRAINT ticket_watchers_ticket_id_fkey,
    ADD CONSTRAINT ticket_watchers_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_category_id_fkey,
    ADD CONSTRAINT tickets_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.ticket_categories (id) ON DELETE SET NULL;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_origin_channel_id_fkey,
    ADD CONSTRAINT tickets_origin_channel_id_fkey FOREIGN KEY (origin_channel_id) REFERENCES public.channels (id) ON DELETE SET NULL;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_recurrence_template_id_fkey,
    ADD CONSTRAINT tickets_recurrence_template_id_fkey FOREIGN KEY (recurrence_template_id) REFERENCES public.tickets (id) ON DELETE SET NULL;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_workflow_state_id_fkey,
    ADD CONSTRAINT tickets_workflow_state_id_fkey FOREIGN KEY (workflow_state_id) REFERENCES public.workflow_states (id);
ALTER TABLE public.user_groups
    DROP CONSTRAINT user_groups_group_id_fkey,
    ADD CONSTRAINT user_groups_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.groups (id) ON DELETE CASCADE;
ALTER TABLE public.user_ticket_views
    DROP CONSTRAINT user_ticket_views_ticket_id_fkey,
    ADD CONSTRAINT user_ticket_views_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets (id) ON DELETE CASCADE;
ALTER TABLE public.webhook_deliveries
    DROP CONSTRAINT webhook_deliveries_webhook_id_fkey,
    ADD CONSTRAINT webhook_deliveries_webhook_id_fkey FOREIGN KEY (webhook_id) REFERENCES public.webhooks (id) ON DELETE CASCADE;
ALTER TABLE public.working_calendar_holidays
    DROP CONSTRAINT working_calendar_holidays_calendar_id_fkey,
    ADD CONSTRAINT working_calendar_holidays_calendar_id_fkey FOREIGN KEY (calendar_id) REFERENCES public.working_calendars (id) ON DELETE CASCADE;
ALTER TABLE public.workspace_notices
    DROP CONSTRAINT workspace_notices_incident_ticket_id_fkey,
    ADD CONSTRAINT workspace_notices_incident_ticket_id_fkey FOREIGN KEY (incident_ticket_id) REFERENCES public.tickets (id) ON DELETE SET NULL;

DROP INDEX public.article_contents_workspace_id_id_key;
DROP INDEX public.asset_groups_workspace_id_id_key;
DROP INDEX public.asset_lifecycle_events_workspace_id_id_key;
DROP INDEX public.asset_models_workspace_id_id_key;
DROP INDEX public.assets_workspace_id_id_key;
DROP INDEX public.assignment_rules_workspace_id_id_key;
DROP INDEX public.canned_responses_workspace_id_id_key;
DROP INDEX public.channels_workspace_id_id_key;
DROP INDEX public.comments_workspace_id_id_key;
DROP INDEX public.cycles_workspace_id_id_key;
DROP INDEX public.documentation_collections_workspace_id_id_key;
DROP INDEX public.documentation_pages_workspace_id_id_key;
DROP INDEX public.groups_workspace_id_id_key;
DROP INDEX public.knowledge_gaps_workspace_id_id_key;
DROP INDEX public.manufacturers_workspace_id_id_key;
DROP INDEX public.notifications_workspace_id_id_key;
DROP INDEX public.plugin_collection_schemas_workspace_id_id_key;
DROP INDEX public.plugins_workspace_id_id_key;
DROP INDEX public.projects_workspace_id_id_key;
DROP INDEX public.rules_workspace_id_id_key;
DROP INDEX public.tags_workspace_id_id_key;
DROP INDEX public.ticket_categories_workspace_id_id_key;
DROP INDEX public.tickets_workspace_id_id_key;
DROP INDEX public.webhooks_workspace_id_id_key;
DROP INDEX public.workflow_states_workspace_id_id_key;
DROP INDEX public.working_calendars_workspace_id_id_key;
