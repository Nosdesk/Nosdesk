-- Foreign keys between workspace tables include the workspace: each
-- (workspace_id, x_id) references the parent's (workspace_id, id), so a row can
-- only reference a row in its own workspace, whichever connection writes it.
--
-- 1. Rows that already reference another workspace's row are repaired or
--    removed, since neither workspace can see them: a ticket's workflow state
--    moves to the matching state in its own workspace, a nullable reference is
--    cleared, and any other row whose required reference crosses is deleted.
--    The migration fails, naming the key, if any remain.
-- 2. Each referenced table gets a unique (workspace_id, id) index.
-- 3. Each key is replaced under the same name and delete action, NOT VALID:
--    new writes are checked at once, and the next migration validates the
--    existing rows without blocking writes.

-- `ON DELETE SET NULL (column)` below needs PostgreSQL 15.
DO $$
BEGIN
    IF current_setting('server_version_num')::int < 150000 THEN
        RAISE EXCEPTION 'Nosdesk needs PostgreSQL 15 or later (17 is recommended); this server runs %',
            current_setting('server_version');
    END IF;
END $$;

-- The cleanup's writes aren't app-level changes to audit, and the audit trigger
-- would otherwise refuse them for want of a workspace (NDX01).
SET LOCAL nosdesk.in_audit_read = 'true';

DO $$
DECLARE
    fk record;
    n bigint;
BEGIN
    -- A ticket's workflow state is required, so it moves rather than goes: to
    -- the matching state in its own workspace (the same category first, then
    -- the default, then the lowest position).
    UPDATE public.tickets t
    SET workflow_state_id = (
        SELECT own.id
        FROM public.workflow_states own
        WHERE own.workspace_id = t.workspace_id
          AND own.archived_at IS NULL
        ORDER BY (own.category = foreign_state.category) DESC, own.is_default DESC, own.position
        LIMIT 1
    )
    FROM public.workflow_states foreign_state
    WHERE foreign_state.id = t.workflow_state_id
      AND foreign_state.workspace_id <> t.workspace_id
      AND EXISTS (
          SELECT 1
          FROM public.workflow_states own
          WHERE own.workspace_id = t.workspace_id
            AND own.archived_at IS NULL
      );
    GET DIAGNOSTICS n = ROW_COUNT;
    IF n > 0 THEN
        RAISE NOTICE 'tickets.workflow_state_id: moved % ticket(s) to a state in their own workspace', n;
    END IF;

    -- Every other single-column key between two workspace tables: clear a
    -- nullable reference, delete a row whose required reference crosses.
    FOR fk IN
        SELECT child.relname AS child, col.attname AS col, parent.relname AS parent, col.attnotnull AS required
        FROM pg_constraint con
        JOIN pg_class child ON child.oid = con.conrelid
        JOIN pg_class parent ON parent.oid = con.confrelid
        JOIN pg_attribute col ON col.attrelid = con.conrelid AND col.attnum = con.conkey[1]
        WHERE con.contype = 'f'
          AND con.connamespace = 'public'::regnamespace
          AND array_length(con.conkey, 1) = 1
          AND col.attname <> 'workspace_id'
          AND EXISTS (SELECT 1 FROM pg_attribute a WHERE a.attrelid = con.conrelid AND a.attname = 'workspace_id' AND NOT a.attisdropped)
          AND EXISTS (SELECT 1 FROM pg_attribute a WHERE a.attrelid = con.confrelid AND a.attname = 'workspace_id' AND NOT a.attisdropped)
          AND NOT (child.relname = 'tickets' AND col.attname = 'workflow_state_id')
        ORDER BY child.relname, col.attname
    LOOP
        IF fk.required THEN
            EXECUTE format(
                'DELETE FROM public.%I c USING public.%I p WHERE p.id = c.%I AND p.workspace_id <> c.workspace_id',
                fk.child, fk.parent, fk.col);
        ELSE
            EXECUTE format(
                'UPDATE public.%I c SET %I = NULL FROM public.%I p WHERE p.id = c.%I AND p.workspace_id <> c.workspace_id',
                fk.child, fk.col, fk.parent, fk.col);
        END IF;
        GET DIAGNOSTICS n = ROW_COUNT;
        IF n > 0 THEN
            RAISE NOTICE '%.%: % row(s) referencing another workspace %',
                fk.child, fk.col, n, CASE WHEN fk.required THEN 'deleted' ELSE 'cleared' END;
        END IF;
    END LOOP;

    -- Nothing may remain, or validating the keys would fail with a bare
    -- constraint error.
    FOR fk IN
        SELECT child.relname AS child, col.attname AS col, parent.relname AS parent
        FROM pg_constraint con
        JOIN pg_class child ON child.oid = con.conrelid
        JOIN pg_class parent ON parent.oid = con.confrelid
        JOIN pg_attribute col ON col.attrelid = con.conrelid AND col.attnum = con.conkey[1]
        WHERE con.contype = 'f'
          AND con.connamespace = 'public'::regnamespace
          AND array_length(con.conkey, 1) = 1
          AND col.attname <> 'workspace_id'
          AND EXISTS (SELECT 1 FROM pg_attribute a WHERE a.attrelid = con.conrelid AND a.attname = 'workspace_id' AND NOT a.attisdropped)
          AND EXISTS (SELECT 1 FROM pg_attribute a WHERE a.attrelid = con.confrelid AND a.attname = 'workspace_id' AND NOT a.attisdropped)
    LOOP
        EXECUTE format(
            'SELECT count(*) FROM public.%I c JOIN public.%I p ON p.id = c.%I WHERE p.workspace_id <> c.workspace_id',
            fk.child, fk.parent, fk.col) INTO n;
        IF n > 0 THEN
            RAISE EXCEPTION '%.% still has % row(s) referencing another workspace''s %',
                fk.child, fk.col, n, fk.parent;
        END IF;
    END LOOP;
END $$;

-- The (workspace_id, id) each key references.
CREATE UNIQUE INDEX article_contents_workspace_id_id_key ON public.article_contents USING btree (workspace_id, id);
CREATE UNIQUE INDEX asset_groups_workspace_id_id_key ON public.asset_groups USING btree (workspace_id, id);
CREATE UNIQUE INDEX asset_lifecycle_events_workspace_id_id_key ON public.asset_lifecycle_events USING btree (workspace_id, id);
CREATE UNIQUE INDEX asset_models_workspace_id_id_key ON public.asset_models USING btree (workspace_id, id);
CREATE UNIQUE INDEX assets_workspace_id_id_key ON public.assets USING btree (workspace_id, id);
CREATE UNIQUE INDEX assignment_rules_workspace_id_id_key ON public.assignment_rules USING btree (workspace_id, id);
CREATE UNIQUE INDEX canned_responses_workspace_id_id_key ON public.canned_responses USING btree (workspace_id, id);
CREATE UNIQUE INDEX channels_workspace_id_id_key ON public.channels USING btree (workspace_id, id);
CREATE UNIQUE INDEX comments_workspace_id_id_key ON public.comments USING btree (workspace_id, id);
CREATE UNIQUE INDEX cycles_workspace_id_id_key ON public.cycles USING btree (workspace_id, id);
CREATE UNIQUE INDEX documentation_collections_workspace_id_id_key ON public.documentation_collections USING btree (workspace_id, id);
CREATE UNIQUE INDEX documentation_pages_workspace_id_id_key ON public.documentation_pages USING btree (workspace_id, id);
CREATE UNIQUE INDEX groups_workspace_id_id_key ON public.groups USING btree (workspace_id, id);
CREATE UNIQUE INDEX knowledge_gaps_workspace_id_id_key ON public.knowledge_gaps USING btree (workspace_id, id);
CREATE UNIQUE INDEX manufacturers_workspace_id_id_key ON public.manufacturers USING btree (workspace_id, id);
CREATE UNIQUE INDEX notifications_workspace_id_id_key ON public.notifications USING btree (workspace_id, id);
CREATE UNIQUE INDEX plugin_collection_schemas_workspace_id_id_key ON public.plugin_collection_schemas USING btree (workspace_id, id);
CREATE UNIQUE INDEX plugins_workspace_id_id_key ON public.plugins USING btree (workspace_id, id);
CREATE UNIQUE INDEX projects_workspace_id_id_key ON public.projects USING btree (workspace_id, id);
CREATE UNIQUE INDEX rules_workspace_id_id_key ON public.rules USING btree (workspace_id, id);
CREATE UNIQUE INDEX tags_workspace_id_id_key ON public.tags USING btree (workspace_id, id);
CREATE UNIQUE INDEX ticket_categories_workspace_id_id_key ON public.ticket_categories USING btree (workspace_id, id);
CREATE UNIQUE INDEX tickets_workspace_id_id_key ON public.tickets USING btree (workspace_id, id);
CREATE UNIQUE INDEX webhooks_workspace_id_id_key ON public.webhooks USING btree (workspace_id, id);
CREATE UNIQUE INDEX workflow_states_workspace_id_id_key ON public.workflow_states USING btree (workspace_id, id);
CREATE UNIQUE INDEX working_calendars_workspace_id_id_key ON public.working_calendars USING btree (workspace_id, id);

-- Each key, same name and delete action, now including the workspace.
ALTER TABLE public.article_content_revisions
    DROP CONSTRAINT article_content_revisions_article_content_id_fkey,
    ADD CONSTRAINT article_content_revisions_article_content_id_fkey FOREIGN KEY (workspace_id, article_content_id) REFERENCES public.article_contents (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.article_contents
    DROP CONSTRAINT article_contents_ticket_id_fkey,
    ADD CONSTRAINT article_contents_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_audits
    DROP CONSTRAINT asset_audits_asset_id_fkey,
    ADD CONSTRAINT asset_audits_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_directory_memberships
    DROP CONSTRAINT device_groups_device_id_fkey,
    ADD CONSTRAINT device_groups_device_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_directory_memberships
    DROP CONSTRAINT device_groups_group_id_fkey,
    ADD CONSTRAINT device_groups_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_disposals
    DROP CONSTRAINT asset_disposals_asset_id_fkey,
    ADD CONSTRAINT asset_disposals_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_disposals
    DROP CONSTRAINT asset_disposals_lifecycle_event_id_fkey,
    ADD CONSTRAINT asset_disposals_lifecycle_event_id_fkey FOREIGN KEY (workspace_id, lifecycle_event_id) REFERENCES public.asset_lifecycle_events (workspace_id, id) ON DELETE SET NULL (lifecycle_event_id) NOT VALID;
ALTER TABLE public.asset_group_assignments
    DROP CONSTRAINT asset_group_assignments_asset_id_fkey,
    ADD CONSTRAINT asset_group_assignments_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_group_assignments
    DROP CONSTRAINT asset_group_assignments_group_id_fkey,
    ADD CONSTRAINT asset_group_assignments_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.asset_groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_lifecycle_events
    DROP CONSTRAINT asset_lifecycle_events_asset_id_fkey,
    ADD CONSTRAINT asset_lifecycle_events_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_lifecycle_events
    DROP CONSTRAINT asset_lifecycle_events_ticket_id_fkey,
    ADD CONSTRAINT asset_lifecycle_events_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_asset_id_fkey,
    ADD CONSTRAINT asset_loans_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_ticket_id_fkey,
    ADD CONSTRAINT asset_loans_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.asset_media
    DROP CONSTRAINT asset_media_asset_id_fkey,
    ADD CONSTRAINT asset_media_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_models
    DROP CONSTRAINT asset_models_manufacturer_id_fkey,
    ADD CONSTRAINT asset_models_manufacturer_id_fkey FOREIGN KEY (workspace_id, manufacturer_id) REFERENCES public.manufacturers (workspace_id, id) ON DELETE RESTRICT NOT VALID;
ALTER TABLE public.asset_usage_log
    DROP CONSTRAINT asset_usage_log_asset_id_fkey,
    ADD CONSTRAINT asset_usage_log_asset_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.asset_usage_log
    DROP CONSTRAINT asset_usage_log_ticket_id_fkey,
    ADD CONSTRAINT asset_usage_log_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.assets
    DROP CONSTRAINT assets_model_id_fkey,
    ADD CONSTRAINT assets_model_id_fkey FOREIGN KEY (workspace_id, model_id) REFERENCES public.asset_models (workspace_id, id) ON DELETE SET NULL (model_id) NOT VALID;
ALTER TABLE public.assignment_log
    DROP CONSTRAINT assignment_log_rule_id_fkey,
    ADD CONSTRAINT assignment_log_rule_id_fkey FOREIGN KEY (workspace_id, rule_id) REFERENCES public.assignment_rules (workspace_id, id) ON DELETE SET NULL (rule_id) NOT VALID;
ALTER TABLE public.assignment_log
    DROP CONSTRAINT assignment_log_ticket_id_fkey,
    ADD CONSTRAINT assignment_log_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.assignment_rule_state
    DROP CONSTRAINT assignment_rule_state_rule_id_fkey,
    ADD CONSTRAINT assignment_rule_state_rule_id_fkey FOREIGN KEY (workspace_id, rule_id) REFERENCES public.assignment_rules (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.assignment_rules
    DROP CONSTRAINT assignment_rules_category_id_fkey,
    ADD CONSTRAINT assignment_rules_category_id_fkey FOREIGN KEY (workspace_id, category_id) REFERENCES public.ticket_categories (workspace_id, id) ON DELETE SET NULL (category_id) NOT VALID;
ALTER TABLE public.assignment_rules
    DROP CONSTRAINT assignment_rules_target_group_id_fkey,
    ADD CONSTRAINT assignment_rules_target_group_id_fkey FOREIGN KEY (workspace_id, target_group_id) REFERENCES public.groups (workspace_id, id) ON DELETE SET NULL (target_group_id) NOT VALID;
ALTER TABLE public.attachments
    DROP CONSTRAINT attachments_comment_id_fkey,
    ADD CONSTRAINT attachments_comment_id_fkey FOREIGN KEY (workspace_id, comment_id) REFERENCES public.comments (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.canned_response_insertions
    DROP CONSTRAINT canned_response_insertions_canned_response_id_fkey,
    ADD CONSTRAINT canned_response_insertions_canned_response_id_fkey FOREIGN KEY (workspace_id, canned_response_id) REFERENCES public.canned_responses (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.canned_response_insertions
    DROP CONSTRAINT canned_response_insertions_ticket_id_fkey,
    ADD CONSTRAINT canned_response_insertions_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.category_approvers
    DROP CONSTRAINT category_approvers_category_id_fkey,
    ADD CONSTRAINT category_approvers_category_id_fkey FOREIGN KEY (workspace_id, category_id) REFERENCES public.ticket_categories (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.category_group_visibility
    DROP CONSTRAINT category_group_visibility_category_id_fkey,
    ADD CONSTRAINT category_group_visibility_category_id_fkey FOREIGN KEY (workspace_id, category_id) REFERENCES public.ticket_categories (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.category_group_visibility
    DROP CONSTRAINT category_group_visibility_group_id_fkey,
    ADD CONSTRAINT category_group_visibility_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.channel_credentials
    DROP CONSTRAINT channel_credentials_channel_id_fkey,
    ADD CONSTRAINT channel_credentials_channel_id_fkey FOREIGN KEY (workspace_id, channel_id) REFERENCES public.channels (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_channel_id_fkey,
    ADD CONSTRAINT channel_messages_channel_id_fkey FOREIGN KEY (workspace_id, channel_id) REFERENCES public.channels (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_comment_id_fkey,
    ADD CONSTRAINT channel_messages_comment_id_fkey FOREIGN KEY (workspace_id, comment_id) REFERENCES public.comments (workspace_id, id) ON DELETE SET NULL (comment_id) NOT VALID;
ALTER TABLE public.channel_messages
    DROP CONSTRAINT channel_messages_ticket_id_fkey,
    ADD CONSTRAINT channel_messages_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.comment_ticket_references
    DROP CONSTRAINT comment_ticket_references_comment_id_fkey,
    ADD CONSTRAINT comment_ticket_references_comment_id_fkey FOREIGN KEY (workspace_id, comment_id) REFERENCES public.comments (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.comment_ticket_references
    DROP CONSTRAINT comment_ticket_references_referenced_ticket_id_fkey,
    ADD CONSTRAINT comment_ticket_references_referenced_ticket_id_fkey FOREIGN KEY (workspace_id, referenced_ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.comments
    DROP CONSTRAINT comments_ticket_id_fkey,
    ADD CONSTRAINT comments_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.cycle_tickets
    DROP CONSTRAINT cycle_tickets_cycle_id_fkey,
    ADD CONSTRAINT cycle_tickets_cycle_id_fkey FOREIGN KEY (workspace_id, cycle_id) REFERENCES public.cycles (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.cycle_tickets
    DROP CONSTRAINT cycle_tickets_ticket_id_fkey,
    ADD CONSTRAINT cycle_tickets_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.cycles
    DROP CONSTRAINT cycles_project_id_fkey,
    ADD CONSTRAINT cycles_project_id_fkey FOREIGN KEY (workspace_id, project_id) REFERENCES public.projects (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_collection_pages
    DROP CONSTRAINT documentation_collection_pages_collection_id_fkey,
    ADD CONSTRAINT documentation_collection_pages_collection_id_fkey FOREIGN KEY (workspace_id, collection_id) REFERENCES public.documentation_collections (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_collection_pages
    DROP CONSTRAINT documentation_collection_pages_page_id_fkey,
    ADD CONSTRAINT documentation_collection_pages_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_collection_id_fkey,
    ADD CONSTRAINT documentation_collection_visibility_collection_id_fkey FOREIGN KEY (workspace_id, collection_id) REFERENCES public.documentation_collections (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_group_id_fkey,
    ADD CONSTRAINT documentation_collection_visibility_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_embeddings
    DROP CONSTRAINT documentation_page_embeddings_source_page_id_fkey,
    ADD CONSTRAINT documentation_page_embeddings_source_page_id_fkey FOREIGN KEY (workspace_id, source_page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_embeddings
    DROP CONSTRAINT documentation_page_embeddings_target_page_id_fkey,
    ADD CONSTRAINT documentation_page_embeddings_target_page_id_fkey FOREIGN KEY (workspace_id, target_page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_tickets
    DROP CONSTRAINT documentation_page_tickets_page_id_fkey,
    ADD CONSTRAINT documentation_page_tickets_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_tickets
    DROP CONSTRAINT documentation_page_tickets_ticket_id_fkey,
    ADD CONSTRAINT documentation_page_tickets_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_group_id_fkey,
    ADD CONSTRAINT documentation_page_visibility_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_page_id_fkey,
    ADD CONSTRAINT documentation_page_visibility_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_pages
    DROP CONSTRAINT documentation_pages_parent_id_fkey,
    ADD CONSTRAINT documentation_pages_parent_id_fkey FOREIGN KEY (workspace_id, parent_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_revisions
    DROP CONSTRAINT documentation_revisions_page_id_fkey,
    ADD CONSTRAINT documentation_revisions_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_starred_pages
    DROP CONSTRAINT documentation_starred_pages_page_id_fkey,
    ADD CONSTRAINT documentation_starred_pages_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.documentation_subscriptions
    DROP CONSTRAINT documentation_subscriptions_page_id_fkey,
    ADD CONSTRAINT documentation_subscriptions_page_id_fkey FOREIGN KEY (workspace_id, page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.group_includes
    DROP CONSTRAINT group_includes_child_group_id_fkey,
    ADD CONSTRAINT group_includes_child_group_id_fkey FOREIGN KEY (workspace_id, child_group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.group_includes
    DROP CONSTRAINT group_includes_parent_group_id_fkey,
    ADD CONSTRAINT group_includes_parent_group_id_fkey FOREIGN KEY (workspace_id, parent_group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.inbound_addresses
    DROP CONSTRAINT inbound_addresses_channel_id_fkey,
    ADD CONSTRAINT inbound_addresses_channel_id_fkey FOREIGN KEY (workspace_id, channel_id) REFERENCES public.channels (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.knowledge_gap_signals
    DROP CONSTRAINT knowledge_gap_signals_gap_id_fkey,
    ADD CONSTRAINT knowledge_gap_signals_gap_id_fkey FOREIGN KEY (workspace_id, gap_id) REFERENCES public.knowledge_gaps (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.knowledge_gaps
    DROP CONSTRAINT knowledge_gaps_draft_page_id_fkey,
    ADD CONSTRAINT knowledge_gaps_draft_page_id_fkey FOREIGN KEY (workspace_id, draft_page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE SET NULL (draft_page_id) NOT VALID;
ALTER TABLE public.knowledge_gaps
    DROP CONSTRAINT knowledge_gaps_resolved_page_id_fkey,
    ADD CONSTRAINT knowledge_gaps_resolved_page_id_fkey FOREIGN KEY (workspace_id, resolved_page_id) REFERENCES public.documentation_pages (workspace_id, id) ON DELETE SET NULL (resolved_page_id) NOT VALID;
ALTER TABLE public.linked_tickets
    DROP CONSTRAINT linked_tickets_linked_ticket_id_fkey,
    ADD CONSTRAINT linked_tickets_linked_ticket_id_fkey FOREIGN KEY (workspace_id, linked_ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.linked_tickets
    DROP CONSTRAINT linked_tickets_ticket_id_fkey,
    ADD CONSTRAINT linked_tickets_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.notification_deliveries
    DROP CONSTRAINT notification_deliveries_notification_id_fkey,
    ADD CONSTRAINT notification_deliveries_notification_id_fkey FOREIGN KEY (workspace_id, notification_id) REFERENCES public.notifications (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_channel_id_fkey,
    ADD CONSTRAINT outbound_emails_channel_id_fkey FOREIGN KEY (workspace_id, channel_id) REFERENCES public.channels (workspace_id, id) NOT VALID;
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_comment_id_fkey,
    ADD CONSTRAINT outbound_emails_comment_id_fkey FOREIGN KEY (workspace_id, comment_id) REFERENCES public.comments (workspace_id, id) ON DELETE SET NULL (comment_id) NOT VALID;
ALTER TABLE public.outbound_emails
    DROP CONSTRAINT outbound_emails_ticket_id_fkey,
    ADD CONSTRAINT outbound_emails_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id) NOT VALID;
ALTER TABLE public.plugin_activity
    DROP CONSTRAINT plugin_activity_plugin_id_fkey,
    ADD CONSTRAINT plugin_activity_plugin_id_fkey FOREIGN KEY (workspace_id, plugin_id) REFERENCES public.plugins (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.plugin_collection_rows
    DROP CONSTRAINT plugin_collection_rows_plugin_id_fkey,
    ADD CONSTRAINT plugin_collection_rows_plugin_id_fkey FOREIGN KEY (workspace_id, plugin_id) REFERENCES public.plugins (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.plugin_collection_rows
    DROP CONSTRAINT plugin_collection_rows_schema_id_fkey,
    ADD CONSTRAINT plugin_collection_rows_schema_id_fkey FOREIGN KEY (workspace_id, schema_id) REFERENCES public.plugin_collection_schemas (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.plugin_collection_schemas
    DROP CONSTRAINT plugin_collection_schemas_plugin_id_fkey,
    ADD CONSTRAINT plugin_collection_schemas_plugin_id_fkey FOREIGN KEY (workspace_id, plugin_id) REFERENCES public.plugins (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.plugin_data
    DROP CONSTRAINT plugin_data_plugin_id_fkey,
    ADD CONSTRAINT plugin_data_plugin_id_fkey FOREIGN KEY (workspace_id, plugin_id) REFERENCES public.plugins (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.project_tickets
    DROP CONSTRAINT project_tickets_project_id_fkey,
    ADD CONSTRAINT project_tickets_project_id_fkey FOREIGN KEY (workspace_id, project_id) REFERENCES public.projects (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.project_tickets
    DROP CONSTRAINT project_tickets_ticket_id_fkey,
    ADD CONSTRAINT project_tickets_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.rule_applications
    DROP CONSTRAINT rule_applications_rule_id_fkey,
    ADD CONSTRAINT rule_applications_rule_id_fkey FOREIGN KEY (workspace_id, rule_id) REFERENCES public.rules (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.rule_applications
    DROP CONSTRAINT rule_applications_ticket_id_fkey,
    ADD CONSTRAINT rule_applications_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.rule_versions
    DROP CONSTRAINT rule_versions_rule_id_fkey,
    ADD CONSTRAINT rule_versions_rule_id_fkey FOREIGN KEY (workspace_id, rule_id) REFERENCES public.rules (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_assignee_group_id_filter_fkey,
    ADD CONSTRAINT sla_policies_assignee_group_id_filter_fkey FOREIGN KEY (workspace_id, assignee_group_id_filter) REFERENCES public.groups (workspace_id, id) ON DELETE SET NULL (assignee_group_id_filter) NOT VALID;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_category_id_filter_fkey,
    ADD CONSTRAINT sla_policies_category_id_filter_fkey FOREIGN KEY (workspace_id, category_id_filter) REFERENCES public.ticket_categories (workspace_id, id) ON DELETE SET NULL (category_id_filter) NOT VALID;
ALTER TABLE public.sla_policies
    DROP CONSTRAINT sla_policies_working_calendar_id_fkey,
    ADD CONSTRAINT sla_policies_working_calendar_id_fkey FOREIGN KEY (workspace_id, working_calendar_id) REFERENCES public.working_calendars (workspace_id, id) ON DELETE SET NULL (working_calendar_id) NOT VALID;
ALTER TABLE public.ticket_approvals
    DROP CONSTRAINT ticket_approvals_ticket_id_fkey,
    ADD CONSTRAINT ticket_approvals_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_assets
    DROP CONSTRAINT ticket_devices_device_id_fkey,
    ADD CONSTRAINT ticket_devices_device_id_fkey FOREIGN KEY (workspace_id, asset_id) REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_assets
    DROP CONSTRAINT ticket_devices_ticket_id_fkey,
    ADD CONSTRAINT ticket_devices_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_merges
    DROP CONSTRAINT ticket_merges_merged_into_ticket_id_fkey,
    ADD CONSTRAINT ticket_merges_merged_into_ticket_id_fkey FOREIGN KEY (workspace_id, merged_into_ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_merges
    DROP CONSTRAINT ticket_merges_ticket_id_fkey,
    ADD CONSTRAINT ticket_merges_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_ratings
    DROP CONSTRAINT ticket_ratings_ticket_id_fkey,
    ADD CONSTRAINT ticket_ratings_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_tags
    DROP CONSTRAINT ticket_tags_tag_id_fkey,
    ADD CONSTRAINT ticket_tags_tag_id_fkey FOREIGN KEY (workspace_id, tag_id) REFERENCES public.tags (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_tags
    DROP CONSTRAINT ticket_tags_ticket_id_fkey,
    ADD CONSTRAINT ticket_tags_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.ticket_watchers
    DROP CONSTRAINT ticket_watchers_ticket_id_fkey,
    ADD CONSTRAINT ticket_watchers_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_category_id_fkey,
    ADD CONSTRAINT tickets_category_id_fkey FOREIGN KEY (workspace_id, category_id) REFERENCES public.ticket_categories (workspace_id, id) ON DELETE SET NULL (category_id) NOT VALID;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_origin_channel_id_fkey,
    ADD CONSTRAINT tickets_origin_channel_id_fkey FOREIGN KEY (workspace_id, origin_channel_id) REFERENCES public.channels (workspace_id, id) ON DELETE SET NULL (origin_channel_id) NOT VALID;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_recurrence_template_id_fkey,
    ADD CONSTRAINT tickets_recurrence_template_id_fkey FOREIGN KEY (workspace_id, recurrence_template_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (recurrence_template_id) NOT VALID;
ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_workflow_state_id_fkey,
    ADD CONSTRAINT tickets_workflow_state_id_fkey FOREIGN KEY (workspace_id, workflow_state_id) REFERENCES public.workflow_states (workspace_id, id) NOT VALID;
ALTER TABLE public.user_groups
    DROP CONSTRAINT user_groups_group_id_fkey,
    ADD CONSTRAINT user_groups_group_id_fkey FOREIGN KEY (workspace_id, group_id) REFERENCES public.groups (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.user_ticket_views
    DROP CONSTRAINT user_ticket_views_ticket_id_fkey,
    ADD CONSTRAINT user_ticket_views_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.webhook_deliveries
    DROP CONSTRAINT webhook_deliveries_webhook_id_fkey,
    ADD CONSTRAINT webhook_deliveries_webhook_id_fkey FOREIGN KEY (workspace_id, webhook_id) REFERENCES public.webhooks (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.working_calendar_holidays
    DROP CONSTRAINT working_calendar_holidays_calendar_id_fkey,
    ADD CONSTRAINT working_calendar_holidays_calendar_id_fkey FOREIGN KEY (workspace_id, calendar_id) REFERENCES public.working_calendars (workspace_id, id) ON DELETE CASCADE NOT VALID;
ALTER TABLE public.workspace_notices
    DROP CONSTRAINT workspace_notices_incident_ticket_id_fkey,
    ADD CONSTRAINT workspace_notices_incident_ticket_id_fkey FOREIGN KEY (workspace_id, incident_ticket_id) REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (incident_ticket_id) NOT VALID;
