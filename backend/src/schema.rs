// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "assignment_method"))]
    pub struct AssignmentMethod;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "documentation_status"))]
    pub struct DocumentationStatus;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_status"))]
    pub struct ProjectStatus;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "rule_application_status"))]
    pub struct RuleApplicationStatus;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "rule_state"))]
    pub struct RuleState;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "rule_trigger_kind"))]
    pub struct RuleTriggerKind;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "sync_aggregate"))]
    pub struct SyncAggregate;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "sync_op"))]
    pub struct SyncOp;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "ticket_priority"))]
    pub struct TicketPriority;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "workflow_state_category"))]
    pub struct WorkflowStateCategory;
}

diesel::table! {
    active_sessions (id) {
        id -> Int4,
        user_uuid -> Uuid,
        #[max_length = 255]
        device_name -> Nullable<Varchar>,
        ip_address -> Nullable<Inet>,
        user_agent -> Nullable<Text>,
        #[max_length = 255]
        location -> Nullable<Varchar>,
        created_at -> Timestamptz,
        last_active -> Timestamptz,
        expires_at -> Timestamptz,
        is_current -> Bool,
        session_id -> Uuid,
    }
}

diesel::table! {
    api_tokens (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 64]
        token_hash -> Varchar,
        #[max_length = 8]
        token_prefix -> Varchar,
        user_uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        scopes -> Nullable<Array<Nullable<Text>>>,
        created_at -> Timestamptz,
        created_by -> Uuid,
        expires_at -> Nullable<Timestamptz>,
        revoked_at -> Nullable<Timestamptz>,
        last_used_at -> Nullable<Timestamptz>,
        last_used_ip -> Nullable<Inet>,
        workspace_id -> Int4,
        is_platform_scoped -> Bool,
    }
}

diesel::table! {
    article_content_revisions (id) {
        id -> Int4,
        article_content_id -> Int4,
        revision_number -> Int4,
        yjs_state_vector -> Bytea,
        yjs_document_content -> Bytea,
        contributed_by -> Array<Nullable<Uuid>>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    article_contents (id) {
        id -> Int4,
        ticket_id -> Nullable<Int4>,
        current_revision_number -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        updated_at -> Timestamptz,
        updated_by -> Nullable<Uuid>,
        yjs_state_vector -> Nullable<Bytea>,
        yjs_document -> Nullable<Bytea>,
        yjs_client_id -> Nullable<Int8>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    asset_audits (id) {
        id -> Int8,
        asset_id -> Int4,
        counted_quantity -> Numeric,
        previous_quantity -> Numeric,
        delta -> Numeric,
        notes -> Nullable<Text>,
        recorded_by -> Nullable<Uuid>,
        recorded_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    asset_groups (asset_id, group_id) {
        asset_id -> Int4,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        #[max_length = 50]
        external_source -> Nullable<Varchar>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    asset_kinds (id) {
        id -> Int4,
        #[max_length = 64]
        slug -> Varchar,
        #[max_length = 255]
        label -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 64]
        icon -> Nullable<Varchar>,
        attribute_schema -> Jsonb,
        sort_order -> Int4,
        is_builtin -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        #[max_length = 16]
        category -> Varchar,
        workspace_id -> Int4,
    }
}

diesel::table! {
    asset_usage_log (id) {
        id -> Int8,
        asset_id -> Int4,
        ticket_id -> Nullable<Int4>,
        quantity_used -> Numeric,
        #[max_length = 32]
        unit -> Varchar,
        recorded_by -> Nullable<Uuid>,
        recorded_at -> Timestamptz,
        notes -> Nullable<Text>,
        #[max_length = 16]
        event_kind -> Varchar,
        workspace_id -> Int4,
    }
}

diesel::table! {
    assets (id) {
        id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        #[max_length = 255]
        serial_number -> Nullable<Varchar>,
        #[max_length = 255]
        manufacturer -> Nullable<Varchar>,
        #[max_length = 255]
        model -> Nullable<Varchar>,
        #[max_length = 255]
        location -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        notes -> Nullable<Text>,
        primary_user_uuid -> Nullable<Uuid>,
        purchase_date -> Nullable<Date>,
        #[max_length = 255]
        asset_tag -> Nullable<Varchar>,
        #[max_length = 64]
        kind -> Varchar,
        attributes -> Jsonb,
        quantity -> Nullable<Numeric>,
        #[max_length = 32]
        unit -> Nullable<Varchar>,
        #[max_length = 32]
        external_sync_source -> Nullable<Varchar>,
        low_stock_threshold -> Nullable<Numeric>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::AssignmentMethod;

    assignment_log (id) {
        id -> Int4,
        ticket_id -> Int4,
        rule_id -> Nullable<Int4>,
        #[max_length = 50]
        trigger_type -> Varchar,
        previous_assignee_uuid -> Nullable<Uuid>,
        new_assignee_uuid -> Nullable<Uuid>,
        method -> AssignmentMethod,
        context -> Nullable<Jsonb>,
        assigned_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    assignment_rule_state (rule_id) {
        rule_id -> Int4,
        last_assigned_index -> Int4,
        total_assignments -> Int4,
        last_assigned_at -> Nullable<Timestamptz>,
        last_assigned_user_uuid -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::AssignmentMethod;

    assignment_rules (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        priority -> Int4,
        is_active -> Bool,
        method -> AssignmentMethod,
        target_user_uuid -> Nullable<Uuid>,
        target_group_id -> Nullable<Int4>,
        trigger_on_create -> Bool,
        trigger_on_category_change -> Bool,
        category_id -> Nullable<Int4>,
        conditions -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    attachments (id) {
        id -> Int4,
        #[max_length = 2048]
        url -> Varchar,
        #[max_length = 255]
        name -> Varchar,
        file_size -> Nullable<Int8>,
        #[max_length = 100]
        mime_type -> Nullable<Varchar>,
        #[max_length = 64]
        checksum -> Nullable<Varchar>,
        comment_id -> Nullable<Int4>,
        uploaded_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        transcription -> Nullable<Text>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    audit_log (id, occurred_at) {
        id -> Int8,
        table_name -> Text,
        pk_text -> Text,
        #[max_length = 1]
        op -> Bpchar,
        before_jsonb -> Nullable<Jsonb>,
        after_jsonb -> Nullable<Jsonb>,
        changed_cols -> Nullable<Array<Nullable<Text>>>,
        actor_uuid -> Nullable<Uuid>,
        correlation_id -> Nullable<Uuid>,
        occurred_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    audit_log_default (id, occurred_at) {
        id -> Int8,
        table_name -> Text,
        pk_text -> Text,
        #[max_length = 1]
        op -> Bpchar,
        before_jsonb -> Nullable<Jsonb>,
        after_jsonb -> Nullable<Jsonb>,
        changed_cols -> Nullable<Array<Nullable<Text>>>,
        actor_uuid -> Nullable<Uuid>,
        correlation_id -> Nullable<Uuid>,
        occurred_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    backup_jobs (id) {
        id -> Uuid,
        #[max_length = 20]
        job_type -> Varchar,
        #[max_length = 20]
        status -> Varchar,
        include_sensitive -> Bool,
        file_path -> Nullable<Text>,
        file_size -> Nullable<Int8>,
        error_message -> Nullable<Text>,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        completed_at -> Nullable<Timestamptz>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    canned_response_insertions (id) {
        id -> Int8,
        canned_response_id -> Int4,
        user_uuid -> Nullable<Uuid>,
        ticket_id -> Nullable<Int4>,
        inserted_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    canned_responses (id) {
        id -> Int4,
        #[max_length = 255]
        title -> Varchar,
        body -> Text,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    category_group_visibility (category_id, group_id) {
        category_id -> Int4,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    channel_credentials (id) {
        id -> Int4,
        channel_id -> Int4,
        #[max_length = 64]
        credential_type -> Varchar,
        expires_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
        encrypted_value -> Bytea,
        encrypted_kek_id -> Int2,
    }
}

diesel::table! {
    channel_messages (id) {
        id -> Int8,
        channel_id -> Int4,
        #[max_length = 998]
        external_id -> Varchar,
        #[max_length = 16]
        direction -> Varchar,
        ticket_id -> Nullable<Int4>,
        comment_id -> Nullable<Int4>,
        #[max_length = 998]
        in_reply_to -> Nullable<Varchar>,
        #[max_length = 320]
        from_address -> Nullable<Varchar>,
        author_user_uuid -> Nullable<Uuid>,
        raw_metadata -> Nullable<Jsonb>,
        received_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    channels (id) {
        id -> Int4,
        #[max_length = 64]
        provider -> Varchar,
        #[max_length = 255]
        name -> Varchar,
        enabled -> Bool,
        config -> Jsonb,
        runtime_state -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        last_polled_at -> Nullable<Timestamptz>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    comments (id) {
        id -> Int4,
        content -> Text,
        ticket_id -> Int4,
        user_uuid -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        is_edited -> Bool,
        edit_count -> Int4,
        channel_metadata -> Nullable<Jsonb>,
        is_internal -> Bool,
        deleted_at -> Nullable<Timestamptz>,
        #[max_length = 16]
        content_format -> Varchar,
        body_text -> Nullable<Text>,
        body_html -> Nullable<Text>,
        new_content -> Nullable<Text>,
        quoted_content -> Nullable<Text>,
        raw_source_uri -> Nullable<Text>,
        workspace_id -> Int4,
        #[max_length = 16]
        render_kind -> Nullable<Varchar>,
    }
}

diesel::table! {
    csp_reports (id) {
        id -> Int8,
        #[max_length = 64]
        dedup_hash -> Bpchar,
        #[max_length = 64]
        effective_directive -> Varchar,
        blocked_uri -> Nullable<Text>,
        source_file -> Nullable<Text>,
        line_number -> Nullable<Int4>,
        column_number -> Nullable<Int4>,
        document_uri -> Text,
        referrer -> Nullable<Text>,
        #[max_length = 64]
        violated_directive -> Nullable<Varchar>,
        original_policy -> Nullable<Text>,
        #[max_length = 16]
        disposition -> Varchar,
        user_agent -> Nullable<Text>,
        user_uuid -> Nullable<Uuid>,
        occurrence_count -> Int4,
        first_seen_at -> Timestamptz,
        last_seen_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    cycle_tickets (cycle_id, ticket_id) {
        cycle_id -> Int4,
        ticket_id -> Int4,
        added_at -> Timestamptz,
        added_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    cycles (id) {
        id -> Int4,
        uuid -> Uuid,
        project_id -> Int4,
        #[max_length = 120]
        name -> Varchar,
        start_at -> Nullable<Timestamptz>,
        end_at -> Nullable<Timestamptz>,
        #[max_length = 20]
        state -> Varchar,
        completion_snapshot -> Nullable<Jsonb>,
        completed_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        archived_at -> Nullable<Timestamptz>,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_collection_pages (collection_id, page_id) {
        collection_id -> Int4,
        page_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_collection_visibility (id) {
        collection_id -> Int4,
        group_id -> Nullable<Int4>,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        id -> Int4,
        user_uuid -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_collections (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        #[max_length = 255]
        slug -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 50]
        icon -> Nullable<Varchar>,
        #[max_length = 7]
        color -> Nullable<Varchar>,
        is_system -> Bool,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        display_order -> Int4,
        description_yjs -> Nullable<Bytea>,
        description_state_vector -> Nullable<Bytea>,
        description_text -> Nullable<Text>,
        hide_titles_from_non_members -> Bool,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_page_embeddings (source_page_id, target_page_id) {
        source_page_id -> Int4,
        target_page_id -> Int4,
        created_at -> Timestamp,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_page_tickets (page_id, ticket_id) {
        page_id -> Int4,
        ticket_id -> Int4,
        #[max_length = 32]
        link_type -> Varchar,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_page_visibility (id) {
        page_id -> Int4,
        group_id -> Nullable<Int4>,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        id -> Int4,
        user_uuid -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::DocumentationStatus;

    documentation_pages (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        title -> Varchar,
        #[max_length = 255]
        slug -> Varchar,
        #[max_length = 50]
        icon -> Nullable<Varchar>,
        #[max_length = 2048]
        cover_image -> Nullable<Varchar>,
        status -> DocumentationStatus,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
        last_edited_by -> Uuid,
        parent_id -> Nullable<Int4>,
        display_order -> Nullable<Int4>,
        is_public -> Bool,
        is_template -> Bool,
        archived_at -> Nullable<Timestamptz>,
        yjs_state_vector -> Nullable<Bytea>,
        yjs_document -> Nullable<Bytea>,
        yjs_client_id -> Nullable<Int8>,
        has_unsaved_changes -> Bool,
        deleted_at -> Nullable<Timestamptz>,
        verified_by -> Nullable<Uuid>,
        verified_at -> Nullable<Timestamptz>,
        verify_interval_days -> Nullable<Int4>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_revisions (id) {
        id -> Int4,
        page_id -> Int4,
        revision_number -> Int4,
        #[max_length = 255]
        title -> Varchar,
        yjs_document_snapshot -> Bytea,
        yjs_state_vector -> Bytea,
        created_at -> Timestamptz,
        created_by -> Uuid,
        change_summary -> Nullable<Text>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_starred_pages (id) {
        id -> Int4,
        user_uuid -> Uuid,
        page_id -> Int4,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    documentation_subscriptions (id) {
        id -> Int4,
        user_uuid -> Uuid,
        page_id -> Int4,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    email_suppressions (email) {
        email -> Text,
        reason -> Text,
        bounce_diagnostic -> Nullable<Text>,
        bounce_count -> Int4,
        created_at -> Timestamptz,
        last_seen_at -> Timestamptz,
        metadata -> Jsonb,
    }
}

diesel::table! {
    group_includes (parent_group_id, child_group_id) {
        parent_group_id -> Int4,
        child_group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    groups (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 7]
        color -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        #[max_length = 255]
        external_id -> Nullable<Varchar>,
        #[max_length = 50]
        external_source -> Nullable<Varchar>,
        #[max_length = 50]
        group_type -> Nullable<Varchar>,
        mail_enabled -> Bool,
        security_enabled -> Bool,
        last_synced_at -> Nullable<Timestamptz>,
        sync_enabled -> Bool,
        workspace_id -> Int4,
    }
}

diesel::table! {
    idempotency_keys (key) {
        key -> Text,
        response_body -> Jsonb,
        response_status -> Int2,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    import_jobs (id) {
        id -> Uuid,
        #[max_length = 32]
        job_type -> Varchar,
        #[max_length = 32]
        status -> Varchar,
        #[max_length = 255]
        filename -> Varchar,
        file_path -> Text,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        completed_at -> Nullable<Timestamptz>,
        summary -> Nullable<Jsonb>,
        records_committed -> Nullable<Int4>,
        error_message -> Nullable<Text>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    knowledge_gap_signals (id) {
        id -> Int8,
        gap_id -> Int8,
        #[max_length = 32]
        signal_type -> Varchar,
        #[max_length = 32]
        source_kind -> Varchar,
        source_ref -> Text,
        payload -> Jsonb,
        confidence -> Int4,
        detected_by -> Nullable<Uuid>,
        detected_at -> Timestamptz,
        dismissed_at -> Nullable<Timestamptz>,
        dismissed_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    knowledge_gaps (id) {
        id -> Int8,
        title -> Text,
        description -> Nullable<Text>,
        #[max_length = 32]
        status -> Varchar,
        assignee_uuid -> Nullable<Uuid>,
        resolved_page_id -> Nullable<Int4>,
        evidence_count -> Int4,
        last_evidence_at -> Nullable<Timestamptz>,
        impact_score -> Int4,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        dismissed_at -> Nullable<Timestamptz>,
        dismissed_by -> Nullable<Uuid>,
        resolved_at -> Nullable<Timestamptz>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    linked_tickets (ticket_id, linked_ticket_id) {
        ticket_id -> Int4,
        linked_ticket_id -> Int4,
        #[max_length = 50]
        relation_type -> Varchar,
        description -> Nullable<Text>,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    notification_preferences (id) {
        id -> Int4,
        user_uuid -> Uuid,
        notification_type_id -> Int4,
        #[max_length = 20]
        channel -> Varchar,
        enabled -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    notification_rate_limits (id) {
        id -> Int4,
        user_uuid -> Uuid,
        notification_type_id -> Int4,
        #[max_length = 50]
        entity_type -> Varchar,
        entity_id -> Int4,
        last_notified_at -> Timestamptz,
    }
}

diesel::table! {
    notification_types (id) {
        id -> Int4,
        #[max_length = 50]
        code -> Varchar,
        #[max_length = 100]
        name -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 50]
        category -> Varchar,
        default_channels -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    notifications (id) {
        id -> Int4,
        uuid -> Uuid,
        user_uuid -> Uuid,
        notification_type_id -> Int4,
        #[max_length = 50]
        entity_type -> Varchar,
        entity_id -> Int4,
        #[max_length = 255]
        title -> Varchar,
        body -> Nullable<Text>,
        metadata -> Nullable<Jsonb>,
        channels_delivered -> Jsonb,
        is_read -> Bool,
        read_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    outbound_emails (id) {
        id -> Int8,
        channel_id -> Nullable<Int4>,
        ticket_id -> Nullable<Int4>,
        comment_id -> Nullable<Int4>,
        recipient -> Text,
        subject -> Text,
        body_text -> Text,
        body_html -> Nullable<Text>,
        message_id -> Text,
        in_reply_to -> Nullable<Text>,
        references_list -> Array<Nullable<Text>>,
        headers_json -> Jsonb,
        status -> Text,
        attempts -> Int4,
        last_error -> Nullable<Text>,
        last_smtp_code -> Nullable<Int4>,
        next_attempt_at -> Timestamptz,
        lease_token -> Nullable<Uuid>,
        lease_expires_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        sent_at -> Nullable<Timestamptz>,
        failed_at -> Nullable<Timestamptz>,
        correlation_id -> Nullable<Uuid>,
        bounced_at -> Nullable<Timestamptz>,
        bounce_recipient -> Nullable<Text>,
        bounce_diagnostic -> Nullable<Text>,
        idempotency_key -> Nullable<Text>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    passkey_credentials (id) {
        id -> Uuid,
        user_uuid -> Uuid,
        credential_id -> Text,
        #[max_length = 100]
        name -> Varchar,
        credential -> Jsonb,
        transports -> Array<Nullable<Text>>,
        backup_eligible -> Bool,
        backup_state -> Bool,
        created_at -> Timestamptz,
        last_used_at -> Nullable<Timestamptz>,
        sign_count -> Int8,
        backup_state_changed_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    plugin_activity (id) {
        id -> Int4,
        uuid -> Uuid,
        plugin_id -> Int4,
        #[max_length = 100]
        action -> Varchar,
        details -> Nullable<Jsonb>,
        user_uuid -> Nullable<Uuid>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    plugin_collection_rows (id) {
        id -> Int4,
        uuid -> Uuid,
        plugin_id -> Int4,
        schema_id -> Int4,
        data -> Jsonb,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    plugin_collection_schemas (id) {
        id -> Int4,
        uuid -> Uuid,
        plugin_id -> Int4,
        #[max_length = 100]
        collection_name -> Varchar,
        schema -> Jsonb,
        version -> Int4,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    plugin_data (id) {
        id -> Int4,
        uuid -> Uuid,
        plugin_id -> Int4,
        #[max_length = 20]
        data_type -> Varchar,
        #[max_length = 255]
        key -> Varchar,
        value -> Nullable<Jsonb>,
        is_secret -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    plugin_local_signing_key (id) {
        id -> Int4,
        pubkey -> Text,
        encrypted_sk -> Bytea,
        #[max_length = 64]
        fingerprint -> Varchar,
        created_at -> Timestamptz,
        encrypted_sk_kek_id -> Int2,
    }
}

diesel::table! {
    plugin_registry_state (id) {
        id -> Int4,
        publishers_version -> Int8,
        index_version -> Int8,
        last_fetched_at -> Nullable<Timestamptz>,
        last_fetch_error -> Nullable<Text>,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    plugin_trusted_publishers (id) {
        id -> Int4,
        pubkey -> Text,
        #[max_length = 200]
        display_name -> Varchar,
        #[max_length = 32]
        tier -> Varchar,
        website -> Nullable<Text>,
        added_at -> Timestamptz,
        revoked_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    plugins (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 100]
        name -> Varchar,
        #[max_length = 255]
        display_name -> Varchar,
        #[max_length = 50]
        version -> Varchar,
        description -> Nullable<Text>,
        manifest -> Jsonb,
        #[max_length = 50]
        trust_level -> Varchar,
        installed_by -> Nullable<Uuid>,
        installed_at -> Timestamptz,
        updated_at -> Timestamptz,
        #[max_length = 64]
        bundle_hash -> Nullable<Varchar>,
        bundle_size -> Nullable<Int4>,
        bundle_uploaded_at -> Nullable<Timestamptz>,
        #[max_length = 20]
        source -> Varchar,
        signer_pubkey -> Nullable<Text>,
        #[max_length = 32]
        signer_source -> Nullable<Varchar>,
        signature_metadata -> Nullable<Jsonb>,
        icon_svg -> Nullable<Bytea>,
        #[max_length = 32]
        state -> Varchar,
        bundle_js -> Nullable<Bytea>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    project_tickets (project_id, ticket_id) {
        project_id -> Int4,
        ticket_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        display_order -> Int4,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ProjectStatus;

    projects (id) {
        id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        status -> ProjectStatus,
        start_date -> Nullable<Date>,
        end_date -> Nullable<Date>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        owner_uuid -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    refresh_tokens (id) {
        id -> Int4,
        #[max_length = 64]
        token_hash -> Varchar,
        user_uuid -> Uuid,
        created_at -> Timestamptz,
        expires_at -> Timestamptz,
        revoked_at -> Nullable<Timestamptz>,
        session_id -> Nullable<Uuid>,
        family_id -> Uuid,
        is_used -> Bool,
        used_at -> Nullable<Timestamptz>,
        #[max_length = 64]
        replaced_by_hash -> Nullable<Varchar>,
        grace_expires_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    reset_tokens (token_hash) {
        #[max_length = 64]
        token_hash -> Varchar,
        user_uuid -> Uuid,
        #[max_length = 50]
        token_type -> Varchar,
        ip_address -> Nullable<Inet>,
        user_agent -> Nullable<Text>,
        created_at -> Timestamptz,
        expires_at -> Timestamptz,
        used_at -> Nullable<Timestamptz>,
        is_used -> Bool,
        metadata -> Nullable<Jsonb>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::RuleApplicationStatus;

    rule_applications (id) {
        id -> Int8,
        workspace_id -> Int4,
        rule_id -> Int4,
        rule_version -> Int4,
        ticket_id -> Int4,
        status -> RuleApplicationStatus,
        correlation_id -> Nullable<Uuid>,
        actor_uuid -> Nullable<Uuid>,
        #[max_length = 16]
        actor_kind -> Varchar,
        originating_event_id -> Nullable<Uuid>,
        #[max_length = 64]
        originating_event_kind -> Nullable<Varchar>,
        condition_evaluation -> Nullable<Jsonb>,
        actions_taken -> Nullable<Jsonb>,
        actions_skipped -> Nullable<Jsonb>,
        failure_reason -> Nullable<Text>,
        applied_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::RuleTriggerKind;
    use super::sql_types::RuleState;

    rule_versions (id) {
        id -> Int4,
        rule_id -> Int4,
        workspace_id -> Int4,
        version -> Int4,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        trigger_kind -> RuleTriggerKind,
        trigger_config -> Jsonb,
        conditions -> Jsonb,
        actions -> Jsonb,
        state -> RuleState,
        priority -> Int4,
        saved_by -> Nullable<Uuid>,
        saved_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::RuleTriggerKind;
    use super::sql_types::RuleState;

    rules (id) {
        id -> Int4,
        workspace_id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        trigger_kind -> RuleTriggerKind,
        trigger_config -> Jsonb,
        conditions -> Jsonb,
        actions -> Jsonb,
        reads_set -> Array<Nullable<Text>>,
        writes_set -> Array<Nullable<Text>>,
        state -> RuleState,
        priority -> Int4,
        last_fired_at -> Nullable<Timestamptz>,
        fire_count -> Int4,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        archived_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    saved_views (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 20]
        scope -> Varchar,
        scope_id -> Nullable<Text>,
        #[max_length = 120]
        name -> Varchar,
        shape -> Jsonb,
        filter -> Jsonb,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        #[max_length = 20]
        dataset -> Varchar,
        workspace_id -> Int4,
    }
}

diesel::table! {
    search_index_state (id) {
        id -> Int4,
        #[max_length = 50]
        entity_type -> Varchar,
        last_indexed_at -> Nullable<Timestamptz>,
        index_version -> Int4,
        document_count -> Int4,
        last_error -> Nullable<Text>,
        last_error_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    search_query_log (id) {
        id -> Int8,
        query_raw -> Text,
        query_norm -> Text,
        result_count -> Int4,
        searched_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    security_events (id) {
        id -> Int4,
        user_uuid -> Nullable<Uuid>,
        #[max_length = 50]
        event_type -> Varchar,
        ip_address -> Nullable<Inet>,
        user_agent -> Nullable<Text>,
        #[max_length = 255]
        location -> Nullable<Varchar>,
        details -> Nullable<Jsonb>,
        #[max_length = 20]
        severity -> Varchar,
        created_at -> Timestamptz,
        session_id -> Nullable<Int4>,
    }
}

diesel::table! {
    site_settings (id) {
        id -> Int4,
        #[max_length = 255]
        app_name -> Varchar,
        #[max_length = 2048]
        logo_url -> Nullable<Varchar>,
        #[max_length = 2048]
        logo_light_url -> Nullable<Varchar>,
        #[max_length = 2048]
        favicon_url -> Nullable<Varchar>,
        #[max_length = 7]
        primary_color -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        updated_by -> Nullable<Uuid>,
        guest_tickets_enabled -> Bool,
        guest_public_docs_enabled -> Bool,
        guest_kb_search_enabled -> Bool,
        guest_ticket_lookup_enabled -> Bool,
        guest_help_page_enabled -> Bool,
        #[max_length = 32]
        guest_ticket_default_priority -> Nullable<Varchar>,
        guest_ticket_rate_limit_per_hour -> Int4,
        guest_ticket_email_verification -> Bool,
        guest_ticket_attachments_enabled -> Bool,
        guest_ticket_intro_message -> Nullable<Text>,
        channel_auto_ack_enabled -> Bool,
        channel_auto_ack_template -> Nullable<Text>,
        feature_flags -> Jsonb,
        default_locale -> Text,
        default_timezone -> Text,
        workspace_id -> Int4,
        signature_default -> Nullable<Text>,
    }
}

diesel::table! {
    sla_policies (id) {
        id -> Int4,
        #[max_length = 120]
        name -> Varchar,
        target_response_minutes -> Nullable<Int4>,
        target_resolution_minutes -> Nullable<Int4>,
        working_calendar_id -> Nullable<Int4>,
        #[max_length = 20]
        priority_filter -> Nullable<Varchar>,
        category_id_filter -> Nullable<Int4>,
        is_default -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
        assignee_group_id_filter -> Nullable<Int4>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::SyncAggregate;
    use super::sql_types::SyncOp;

    sync_actions (sync_id, occurred_at) {
        sync_id -> Int8,
        event_uuid -> Uuid,
        aggregate -> SyncAggregate,
        aggregate_id -> Text,
        op -> SyncOp,
        #[max_length = 64]
        event_type -> Varchar,
        schema_version -> Int2,
        data -> Jsonb,
        groups -> Array<Nullable<Text>>,
        actor_uuid -> Nullable<Uuid>,
        #[max_length = 16]
        actor_kind -> Varchar,
        actor_ref -> Nullable<Text>,
        correlation_id -> Nullable<Uuid>,
        causation_id -> Nullable<Uuid>,
        client_tx_id -> Nullable<Text>,
        occurred_at -> Timestamptz,
        recorded_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::SyncAggregate;
    use super::sql_types::SyncOp;

    sync_actions_default (sync_id, occurred_at) {
        sync_id -> Int8,
        event_uuid -> Uuid,
        aggregate -> SyncAggregate,
        aggregate_id -> Text,
        op -> SyncOp,
        #[max_length = 64]
        event_type -> Varchar,
        schema_version -> Int2,
        data -> Jsonb,
        groups -> Array<Nullable<Text>>,
        actor_uuid -> Nullable<Uuid>,
        #[max_length = 16]
        actor_kind -> Varchar,
        actor_ref -> Nullable<Text>,
        correlation_id -> Nullable<Uuid>,
        causation_id -> Nullable<Uuid>,
        client_tx_id -> Nullable<Text>,
        occurred_at -> Timestamptz,
        recorded_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    sync_delta_tokens (id) {
        id -> Int4,
        #[max_length = 50]
        provider_type -> Varchar,
        #[max_length = 50]
        entity_type -> Varchar,
        delta_link -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    sync_history (id) {
        id -> Int4,
        #[max_length = 100]
        sync_type -> Varchar,
        #[max_length = 50]
        status -> Varchar,
        started_at -> Timestamptz,
        completed_at -> Nullable<Timestamptz>,
        error_message -> Nullable<Text>,
        records_processed -> Nullable<Int4>,
        records_created -> Nullable<Int4>,
        records_updated -> Nullable<Int4>,
        records_failed -> Nullable<Int4>,
        #[max_length = 255]
        tenant_id -> Nullable<Varchar>,
        initiated_by -> Nullable<Uuid>,
        is_delta -> Bool,
        workspace_id -> Int4,
    }
}

diesel::table! {
    system_meta (key) {
        key -> Text,
        value -> Jsonb,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    tags (id) {
        id -> Int4,
        #[max_length = 64]
        name -> Varchar,
        #[max_length = 32]
        color -> Nullable<Varchar>,
        description -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        archived_at -> Nullable<Timestamptz>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    ticket_assets (ticket_id, asset_id) {
        ticket_id -> Int4,
        asset_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    ticket_categories (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        description -> Nullable<Text>,
        #[max_length = 7]
        color -> Nullable<Varchar>,
        #[max_length = 50]
        icon -> Nullable<Varchar>,
        display_order -> Int4,
        is_active -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    ticket_rule_runs (event_id, ticket_id, rule_id) {
        event_id -> Uuid,
        ticket_id -> Int4,
        rule_id -> Int4,
        fired_at -> Timestamptz,
    }
}

diesel::table! {
    ticket_tags (ticket_id, tag_id) {
        ticket_id -> Int4,
        tag_id -> Int4,
        created_by -> Nullable<Uuid>,
        created_at -> Timestamptz,
        workspace_id -> Int4,
    }
}

diesel::table! {
    ticket_watchers (ticket_id, user_uuid) {
        ticket_id -> Int4,
        user_uuid -> Uuid,
        created_at -> Timestamptz,
        auto_added -> Bool,
        notify_on_internal_notes -> Bool,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::TicketPriority;

    tickets (id) {
        id -> Int4,
        #[max_length = 255]
        title -> Varchar,
        priority -> TicketPriority,
        requester_uuid -> Nullable<Uuid>,
        assignee_uuid -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        closed_at -> Nullable<Timestamptz>,
        closed_by -> Nullable<Uuid>,
        category_id -> Nullable<Int4>,
        #[max_length = 32]
        submitted_via -> Nullable<Varchar>,
        guest_lookup_token -> Nullable<Uuid>,
        #[max_length = 32]
        verification_state -> Nullable<Varchar>,
        origin_channel_id -> Nullable<Int4>,
        workflow_state_id -> Int4,
        #[max_length = 20]
        triage_state -> Nullable<Varchar>,
        due_date -> Nullable<Timestamptz>,
        recurrence_rule -> Nullable<Text>,
        recurrence_template_id -> Nullable<Int4>,
        resolution_notes -> Nullable<Text>,
        workspace_id -> Int4,
        first_response_at -> Nullable<Timestamptz>,
        sla_response_target_at -> Nullable<Timestamptz>,
        sla_response_breached_at -> Nullable<Timestamptz>,
        sla_resolution_target_at -> Nullable<Timestamptz>,
        sla_resolution_breached_at -> Nullable<Timestamptz>,
        merged_into_ticket_id -> Nullable<Int4>,
        merged_at -> Nullable<Timestamptz>,
        merged_by_user_uuid -> Nullable<Uuid>,
        merge_reason -> Nullable<Text>,
    }
}

diesel::table! {
    user_auth_identities (id) {
        id -> Int4,
        user_uuid -> Uuid,
        #[max_length = 50]
        provider_type -> Varchar,
        #[max_length = 255]
        external_id -> Varchar,
        #[max_length = 320]
        email -> Nullable<Varchar>,
        metadata -> Nullable<Jsonb>,
        #[max_length = 255]
        password_hash -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
    }
}

diesel::table! {
    user_emails (id) {
        id -> Int4,
        user_uuid -> Uuid,
        #[max_length = 320]
        email -> Varchar,
        #[max_length = 50]
        email_type -> Varchar,
        is_primary -> Bool,
        is_verified -> Bool,
        #[max_length = 50]
        source -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
    }
}

diesel::table! {
    user_groups (user_uuid, group_id) {
        user_uuid -> Uuid,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    user_preferences (user_uuid) {
        user_uuid -> Uuid,
        #[max_length = 50]
        theme -> Nullable<Varchar>,
        signature -> Nullable<Text>,
        dashboard_layout -> Nullable<Jsonb>,
        locale -> Nullable<Text>,
        timezone -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    user_recovery_codes (id) {
        id -> Int8,
        user_uuid -> Uuid,
        code_hash -> Text,
        used_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    user_ticket_views (id) {
        id -> Int4,
        user_uuid -> Uuid,
        ticket_id -> Int4,
        first_viewed_at -> Timestamptz,
        last_viewed_at -> Timestamptz,
        view_count -> Int4,
        workspace_id -> Int4,
    }
}

diesel::table! {
    users (uuid) {
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        password_changed_at -> Nullable<Timestamptz>,
        #[max_length = 100]
        pronouns -> Nullable<Varchar>,
        #[max_length = 2048]
        avatar_url -> Nullable<Varchar>,
        #[max_length = 2048]
        banner_url -> Nullable<Varchar>,
        #[max_length = 2048]
        avatar_thumb -> Nullable<Varchar>,
        microsoft_uuid -> Nullable<Uuid>,
        mfa_enabled -> Bool,
        feature_flag_overrides -> Jsonb,
        deleted_at -> Nullable<Timestamptz>,
        mfa_secret -> Nullable<Bytea>,
        mfa_secret_kek_id -> Nullable<Int2>,
        #[max_length = 32]
        platform_role -> Varchar,
    }
}

diesel::table! {
    webhook_deliveries (id) {
        id -> Int4,
        uuid -> Uuid,
        webhook_id -> Int4,
        #[max_length = 100]
        event_type -> Varchar,
        payload -> Jsonb,
        request_headers -> Nullable<Jsonb>,
        response_status -> Nullable<Int4>,
        response_body -> Nullable<Text>,
        response_headers -> Nullable<Jsonb>,
        attempt_number -> Int4,
        duration_ms -> Nullable<Int4>,
        error_message -> Nullable<Text>,
        delivered_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        next_retry_at -> Nullable<Timestamptz>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    webhooks (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        url -> Text,
        #[max_length = 255]
        secret -> Varchar,
        events -> Array<Nullable<Text>>,
        enabled -> Bool,
        headers -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        last_triggered_at -> Nullable<Timestamptz>,
        failure_count -> Int4,
        disabled_reason -> Nullable<Text>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::WorkflowStateCategory;

    workflow_states (id) {
        id -> Int4,
        #[max_length = 64]
        name -> Varchar,
        category -> WorkflowStateCategory,
        #[max_length = 20]
        color -> Varchar,
        position -> Int4,
        is_default -> Bool,
        archived_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
        pauses_sla -> Bool,
    }
}

diesel::table! {
    working_calendar_holidays (id) {
        id -> Int4,
        calendar_id -> Int4,
        date -> Date,
        #[max_length = 120]
        label -> Nullable<Varchar>,
        workspace_id -> Int4,
        #[max_length = 20]
        recurrence -> Varchar,
    }
}

diesel::table! {
    working_calendars (id) {
        id -> Int4,
        #[max_length = 120]
        name -> Varchar,
        #[max_length = 64]
        timezone -> Varchar,
        schedule -> Jsonb,
        is_default -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        workspace_id -> Int4,
    }
}

diesel::table! {
    workspace_members (workspace_id, user_uuid) {
        workspace_id -> Int4,
        user_uuid -> Uuid,
        #[max_length = 32]
        role -> Varchar,
        invited_at -> Timestamptz,
        accepted_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    workspaces (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 64]
        slug -> Varchar,
        #[max_length = 200]
        name -> Varchar,
        #[max_length = 32]
        plan -> Varchar,
        settings -> Jsonb,
        created_at -> Timestamptz,
        archived_at -> Nullable<Timestamptz>,
        organisation_id -> Nullable<Int4>,
        custom_domain -> Nullable<Text>,
    }
}

diesel::table! {
    yjs_snapshots (id) {
        id -> Int8,
        workspace_id -> Int4,
        document_id -> Text,
        snapshot -> Bytea,
        state_vector -> Bytea,
        created_at -> Timestamptz,
    }
}

diesel::joinable!(active_sessions -> users (user_uuid));
diesel::joinable!(api_tokens -> workspaces (workspace_id));
diesel::joinable!(article_content_revisions -> article_contents (article_content_id));
diesel::joinable!(article_content_revisions -> workspaces (workspace_id));
diesel::joinable!(article_contents -> tickets (ticket_id));
diesel::joinable!(article_contents -> workspaces (workspace_id));
diesel::joinable!(asset_audits -> assets (asset_id));
diesel::joinable!(asset_audits -> users (recorded_by));
diesel::joinable!(asset_audits -> workspaces (workspace_id));
diesel::joinable!(asset_groups -> assets (asset_id));
diesel::joinable!(asset_groups -> groups (group_id));
diesel::joinable!(asset_groups -> users (created_by));
diesel::joinable!(asset_groups -> workspaces (workspace_id));
diesel::joinable!(asset_kinds -> users (created_by));
diesel::joinable!(asset_kinds -> workspaces (workspace_id));
diesel::joinable!(asset_usage_log -> assets (asset_id));
diesel::joinable!(asset_usage_log -> tickets (ticket_id));
diesel::joinable!(asset_usage_log -> users (recorded_by));
diesel::joinable!(asset_usage_log -> workspaces (workspace_id));
diesel::joinable!(assets -> workspaces (workspace_id));
diesel::joinable!(assignment_log -> assignment_rules (rule_id));
diesel::joinable!(assignment_log -> tickets (ticket_id));
diesel::joinable!(assignment_log -> workspaces (workspace_id));
diesel::joinable!(assignment_rule_state -> assignment_rules (rule_id));
diesel::joinable!(assignment_rule_state -> users (last_assigned_user_uuid));
diesel::joinable!(assignment_rule_state -> workspaces (workspace_id));
diesel::joinable!(assignment_rules -> groups (target_group_id));
diesel::joinable!(assignment_rules -> ticket_categories (category_id));
diesel::joinable!(assignment_rules -> workspaces (workspace_id));
diesel::joinable!(attachments -> comments (comment_id));
diesel::joinable!(attachments -> users (uploaded_by));
diesel::joinable!(attachments -> workspaces (workspace_id));
diesel::joinable!(audit_log -> workspaces (workspace_id));
diesel::joinable!(audit_log_default -> workspaces (workspace_id));
diesel::joinable!(backup_jobs -> users (created_by));
diesel::joinable!(backup_jobs -> workspaces (workspace_id));
diesel::joinable!(canned_response_insertions -> canned_responses (canned_response_id));
diesel::joinable!(canned_response_insertions -> tickets (ticket_id));
diesel::joinable!(canned_response_insertions -> users (user_uuid));
diesel::joinable!(canned_response_insertions -> workspaces (workspace_id));
diesel::joinable!(canned_responses -> users (created_by));
diesel::joinable!(canned_responses -> workspaces (workspace_id));
diesel::joinable!(category_group_visibility -> groups (group_id));
diesel::joinable!(category_group_visibility -> ticket_categories (category_id));
diesel::joinable!(category_group_visibility -> users (created_by));
diesel::joinable!(category_group_visibility -> workspaces (workspace_id));
diesel::joinable!(channel_credentials -> channels (channel_id));
diesel::joinable!(channel_credentials -> workspaces (workspace_id));
diesel::joinable!(channel_messages -> channels (channel_id));
diesel::joinable!(channel_messages -> comments (comment_id));
diesel::joinable!(channel_messages -> tickets (ticket_id));
diesel::joinable!(channel_messages -> users (author_user_uuid));
diesel::joinable!(channel_messages -> workspaces (workspace_id));
diesel::joinable!(channels -> workspaces (workspace_id));
diesel::joinable!(comments -> tickets (ticket_id));
diesel::joinable!(comments -> users (user_uuid));
diesel::joinable!(comments -> workspaces (workspace_id));
diesel::joinable!(csp_reports -> users (user_uuid));
diesel::joinable!(csp_reports -> workspaces (workspace_id));
diesel::joinable!(cycle_tickets -> cycles (cycle_id));
diesel::joinable!(cycle_tickets -> tickets (ticket_id));
diesel::joinable!(cycle_tickets -> users (added_by));
diesel::joinable!(cycle_tickets -> workspaces (workspace_id));
diesel::joinable!(cycles -> projects (project_id));
diesel::joinable!(cycles -> users (created_by));
diesel::joinable!(cycles -> workspaces (workspace_id));
diesel::joinable!(documentation_collection_pages -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_collection_pages -> users (created_by));
diesel::joinable!(documentation_collection_pages -> workspaces (workspace_id));
diesel::joinable!(documentation_collection_visibility -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_visibility -> groups (group_id));
diesel::joinable!(documentation_collection_visibility -> workspaces (workspace_id));
diesel::joinable!(documentation_collections -> users (created_by));
diesel::joinable!(documentation_collections -> workspaces (workspace_id));
diesel::joinable!(documentation_page_embeddings -> workspaces (workspace_id));
diesel::joinable!(documentation_page_tickets -> documentation_pages (page_id));
diesel::joinable!(documentation_page_tickets -> tickets (ticket_id));
diesel::joinable!(documentation_page_tickets -> users (created_by));
diesel::joinable!(documentation_page_tickets -> workspaces (workspace_id));
diesel::joinable!(documentation_page_visibility -> documentation_pages (page_id));
diesel::joinable!(documentation_page_visibility -> groups (group_id));
diesel::joinable!(documentation_page_visibility -> workspaces (workspace_id));
diesel::joinable!(documentation_pages -> workspaces (workspace_id));
diesel::joinable!(documentation_revisions -> documentation_pages (page_id));
diesel::joinable!(documentation_revisions -> users (created_by));
diesel::joinable!(documentation_revisions -> workspaces (workspace_id));
diesel::joinable!(documentation_starred_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_starred_pages -> users (user_uuid));
diesel::joinable!(documentation_starred_pages -> workspaces (workspace_id));
diesel::joinable!(documentation_subscriptions -> documentation_pages (page_id));
diesel::joinable!(documentation_subscriptions -> users (user_uuid));
diesel::joinable!(documentation_subscriptions -> workspaces (workspace_id));
diesel::joinable!(group_includes -> users (created_by));
diesel::joinable!(group_includes -> workspaces (workspace_id));
diesel::joinable!(groups -> users (created_by));
diesel::joinable!(groups -> workspaces (workspace_id));
diesel::joinable!(import_jobs -> users (created_by));
diesel::joinable!(import_jobs -> workspaces (workspace_id));
diesel::joinable!(knowledge_gap_signals -> knowledge_gaps (gap_id));
diesel::joinable!(knowledge_gap_signals -> workspaces (workspace_id));
diesel::joinable!(knowledge_gaps -> documentation_pages (resolved_page_id));
diesel::joinable!(knowledge_gaps -> workspaces (workspace_id));
diesel::joinable!(linked_tickets -> users (created_by));
diesel::joinable!(linked_tickets -> workspaces (workspace_id));
diesel::joinable!(notification_preferences -> notification_types (notification_type_id));
diesel::joinable!(notification_preferences -> users (user_uuid));
diesel::joinable!(notification_preferences -> workspaces (workspace_id));
diesel::joinable!(notification_rate_limits -> notification_types (notification_type_id));
diesel::joinable!(notification_rate_limits -> users (user_uuid));
diesel::joinable!(notifications -> notification_types (notification_type_id));
diesel::joinable!(notifications -> users (user_uuid));
diesel::joinable!(notifications -> workspaces (workspace_id));
diesel::joinable!(outbound_emails -> channels (channel_id));
diesel::joinable!(outbound_emails -> comments (comment_id));
diesel::joinable!(outbound_emails -> tickets (ticket_id));
diesel::joinable!(outbound_emails -> workspaces (workspace_id));
diesel::joinable!(passkey_credentials -> users (user_uuid));
diesel::joinable!(plugin_activity -> plugins (plugin_id));
diesel::joinable!(plugin_activity -> users (user_uuid));
diesel::joinable!(plugin_activity -> workspaces (workspace_id));
diesel::joinable!(plugin_collection_rows -> plugin_collection_schemas (schema_id));
diesel::joinable!(plugin_collection_rows -> plugins (plugin_id));
diesel::joinable!(plugin_collection_rows -> users (created_by));
diesel::joinable!(plugin_collection_rows -> workspaces (workspace_id));
diesel::joinable!(plugin_collection_schemas -> plugins (plugin_id));
diesel::joinable!(plugin_collection_schemas -> workspaces (workspace_id));
diesel::joinable!(plugin_data -> plugins (plugin_id));
diesel::joinable!(plugin_data -> workspaces (workspace_id));
diesel::joinable!(plugins -> users (installed_by));
diesel::joinable!(plugins -> workspaces (workspace_id));
diesel::joinable!(project_tickets -> projects (project_id));
diesel::joinable!(project_tickets -> tickets (ticket_id));
diesel::joinable!(project_tickets -> users (created_by));
diesel::joinable!(project_tickets -> workspaces (workspace_id));
diesel::joinable!(projects -> workspaces (workspace_id));
diesel::joinable!(refresh_tokens -> users (user_uuid));
diesel::joinable!(reset_tokens -> users (user_uuid));
diesel::joinable!(rule_applications -> rules (rule_id));
diesel::joinable!(rule_applications -> tickets (ticket_id));
diesel::joinable!(rule_applications -> users (actor_uuid));
diesel::joinable!(rule_applications -> workspaces (workspace_id));
diesel::joinable!(rule_versions -> rules (rule_id));
diesel::joinable!(rule_versions -> users (saved_by));
diesel::joinable!(rule_versions -> workspaces (workspace_id));
diesel::joinable!(rules -> users (created_by));
diesel::joinable!(rules -> workspaces (workspace_id));
diesel::joinable!(saved_views -> users (created_by));
diesel::joinable!(saved_views -> workspaces (workspace_id));
diesel::joinable!(search_query_log -> workspaces (workspace_id));
diesel::joinable!(security_events -> active_sessions (session_id));
diesel::joinable!(security_events -> users (user_uuid));
diesel::joinable!(site_settings -> users (updated_by));
diesel::joinable!(site_settings -> workspaces (workspace_id));
diesel::joinable!(sla_policies -> groups (assignee_group_id_filter));
diesel::joinable!(sla_policies -> ticket_categories (category_id_filter));
diesel::joinable!(sla_policies -> users (created_by));
diesel::joinable!(sla_policies -> working_calendars (working_calendar_id));
diesel::joinable!(sla_policies -> workspaces (workspace_id));
diesel::joinable!(sync_actions -> workspaces (workspace_id));
diesel::joinable!(sync_actions_default -> workspaces (workspace_id));
diesel::joinable!(sync_delta_tokens -> workspaces (workspace_id));
diesel::joinable!(sync_history -> users (initiated_by));
diesel::joinable!(sync_history -> workspaces (workspace_id));
diesel::joinable!(tags -> workspaces (workspace_id));
diesel::joinable!(ticket_assets -> assets (asset_id));
diesel::joinable!(ticket_assets -> tickets (ticket_id));
diesel::joinable!(ticket_assets -> users (created_by));
diesel::joinable!(ticket_assets -> workspaces (workspace_id));
diesel::joinable!(ticket_categories -> users (created_by));
diesel::joinable!(ticket_categories -> workspaces (workspace_id));
diesel::joinable!(ticket_tags -> tags (tag_id));
diesel::joinable!(ticket_tags -> tickets (ticket_id));
diesel::joinable!(ticket_tags -> users (created_by));
diesel::joinable!(ticket_tags -> workspaces (workspace_id));
diesel::joinable!(ticket_watchers -> tickets (ticket_id));
diesel::joinable!(ticket_watchers -> users (user_uuid));
diesel::joinable!(ticket_watchers -> workspaces (workspace_id));
diesel::joinable!(tickets -> channels (origin_channel_id));
diesel::joinable!(tickets -> ticket_categories (category_id));
diesel::joinable!(tickets -> workflow_states (workflow_state_id));
diesel::joinable!(tickets -> workspaces (workspace_id));
diesel::joinable!(user_groups -> groups (group_id));
diesel::joinable!(user_groups -> workspaces (workspace_id));
diesel::joinable!(user_preferences -> users (user_uuid));
diesel::joinable!(user_recovery_codes -> users (user_uuid));
diesel::joinable!(user_ticket_views -> tickets (ticket_id));
diesel::joinable!(user_ticket_views -> users (user_uuid));
diesel::joinable!(user_ticket_views -> workspaces (workspace_id));
diesel::joinable!(webhook_deliveries -> webhooks (webhook_id));
diesel::joinable!(webhook_deliveries -> workspaces (workspace_id));
diesel::joinable!(webhooks -> users (created_by));
diesel::joinable!(webhooks -> workspaces (workspace_id));
diesel::joinable!(workflow_states -> users (created_by));
diesel::joinable!(workflow_states -> workspaces (workspace_id));
diesel::joinable!(working_calendar_holidays -> working_calendars (calendar_id));
diesel::joinable!(working_calendar_holidays -> workspaces (workspace_id));
diesel::joinable!(working_calendars -> users (created_by));
diesel::joinable!(working_calendars -> workspaces (workspace_id));
diesel::joinable!(workspace_members -> users (user_uuid));
diesel::joinable!(workspace_members -> workspaces (workspace_id));
diesel::joinable!(yjs_snapshots -> workspaces (workspace_id));

diesel::allow_tables_to_appear_in_same_query!(
    active_sessions,
    api_tokens,
    article_content_revisions,
    article_contents,
    asset_audits,
    asset_groups,
    asset_kinds,
    asset_usage_log,
    assets,
    assignment_log,
    assignment_rule_state,
    assignment_rules,
    attachments,
    audit_log,
    audit_log_default,
    backup_jobs,
    canned_response_insertions,
    canned_responses,
    category_group_visibility,
    channel_credentials,
    channel_messages,
    channels,
    comments,
    csp_reports,
    cycle_tickets,
    cycles,
    documentation_collection_pages,
    documentation_collection_visibility,
    documentation_collections,
    documentation_page_embeddings,
    documentation_page_tickets,
    documentation_page_visibility,
    documentation_pages,
    documentation_revisions,
    documentation_starred_pages,
    documentation_subscriptions,
    email_suppressions,
    group_includes,
    groups,
    idempotency_keys,
    import_jobs,
    knowledge_gap_signals,
    knowledge_gaps,
    linked_tickets,
    notification_preferences,
    notification_rate_limits,
    notification_types,
    notifications,
    outbound_emails,
    passkey_credentials,
    plugin_activity,
    plugin_collection_rows,
    plugin_collection_schemas,
    plugin_data,
    plugin_local_signing_key,
    plugin_registry_state,
    plugin_trusted_publishers,
    plugins,
    project_tickets,
    projects,
    refresh_tokens,
    reset_tokens,
    rule_applications,
    rule_versions,
    rules,
    saved_views,
    search_index_state,
    search_query_log,
    security_events,
    site_settings,
    sla_policies,
    sync_actions,
    sync_actions_default,
    sync_delta_tokens,
    sync_history,
    system_meta,
    tags,
    ticket_assets,
    ticket_categories,
    ticket_rule_runs,
    ticket_tags,
    ticket_watchers,
    tickets,
    user_auth_identities,
    user_emails,
    user_groups,
    user_preferences,
    user_recovery_codes,
    user_ticket_views,
    users,
    webhook_deliveries,
    webhooks,
    workflow_states,
    working_calendar_holidays,
    working_calendars,
    workspace_members,
    workspaces,
    yjs_snapshots,
);
