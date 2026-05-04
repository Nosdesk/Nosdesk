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
    #[diesel(postgres_type(name = "sync_aggregate"))]
    pub struct SyncAggregate;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "sync_op"))]
    pub struct SyncOp;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "ticket_priority"))]
    pub struct TicketPriority;

    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;

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
    }
}

diesel::table! {
    assignment_rule_state (rule_id) {
        rule_id -> Int4,
        last_assigned_index -> Int4,
        total_assignments -> Int4,
        last_assigned_at -> Nullable<Timestamptz>,
        last_assigned_user_uuid -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    category_group_visibility (category_id, group_id) {
        category_id -> Int4,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
    }
}

diesel::table! {
    channel_credentials (id) {
        id -> Int4,
        channel_id -> Int4,
        #[max_length = 64]
        credential_type -> Varchar,
        encrypted_value -> Text,
        expires_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
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
    }
}

diesel::table! {
    cycle_tickets (cycle_id, ticket_id) {
        cycle_id -> Int4,
        ticket_id -> Int4,
        added_at -> Timestamptz,
        added_by -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    device_groups (device_id, group_id) {
        device_id -> Int4,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        #[max_length = 50]
        external_source -> Nullable<Varchar>,
    }
}

diesel::table! {
    devices (id) {
        id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        #[max_length = 255]
        hostname -> Nullable<Varchar>,
        #[max_length = 100]
        device_type -> Nullable<Varchar>,
        #[max_length = 255]
        serial_number -> Nullable<Varchar>,
        #[max_length = 255]
        manufacturer -> Nullable<Varchar>,
        #[max_length = 255]
        model -> Nullable<Varchar>,
        #[max_length = 50]
        warranty_status -> Nullable<Varchar>,
        #[max_length = 255]
        location -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        notes -> Nullable<Text>,
        primary_user_uuid -> Nullable<Uuid>,
        #[max_length = 255]
        microsoft_device_id -> Nullable<Varchar>,
        #[max_length = 255]
        intune_device_id -> Nullable<Varchar>,
        #[max_length = 255]
        entra_device_id -> Nullable<Varchar>,
        #[max_length = 50]
        compliance_state -> Nullable<Varchar>,
        last_sync_time -> Nullable<Timestamptz>,
        #[max_length = 100]
        operating_system -> Nullable<Varchar>,
        #[max_length = 100]
        os_version -> Nullable<Varchar>,
        is_managed -> Nullable<Bool>,
        enrollment_date -> Nullable<Timestamptz>,
        warranty_start_date -> Nullable<Date>,
        warranty_end_date -> Nullable<Date>,
        purchase_date -> Nullable<Date>,
        #[max_length = 255]
        asset_tag -> Nullable<Varchar>,
    }
}

diesel::table! {
    documentation_collection_pages (collection_id, page_id) {
        collection_id -> Int4,
        page_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    documentation_page_embeddings (source_page_id, target_page_id) {
        source_page_id -> Int4,
        target_page_id -> Int4,
        created_at -> Timestamp,
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
    }
}

diesel::table! {
    documentation_starred_pages (id) {
        id -> Int4,
        user_uuid -> Uuid,
        page_id -> Int4,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    documentation_subscriptions (id) {
        id -> Int4,
        user_uuid -> Uuid,
        page_id -> Int4,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    group_includes (parent_group_id, child_group_id) {
        parent_group_id -> Int4,
        child_group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    project_tickets (project_id, ticket_id) {
        project_id -> Int4,
        ticket_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        display_order -> Int4,
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
        is_default -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        archived_at -> Nullable<Timestamptz>,
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
    }
}

diesel::table! {
    working_calendar_holidays (id) {
        id -> Int4,
        calendar_id -> Int4,
        date -> Date,
        #[max_length = 120]
        label -> Nullable<Varchar>,
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
    }
}

diesel::table! {
    search_query_log (id) {
        id -> Int8,
        query_raw -> Text,
        query_norm -> Text,
        result_count -> Int4,
        searched_at -> Timestamptz,
    }
}

diesel::table! {
    security_events (id) {
        id -> Int4,
        user_uuid -> Uuid,
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
    }
}

diesel::table! {
    ticket_devices (ticket_id, device_id) {
        ticket_id -> Int4,
        device_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::UserRole;

    users (uuid) {
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        role -> UserRole,
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
        #[max_length = 50]
        theme -> Nullable<Varchar>,
        microsoft_uuid -> Nullable<Uuid>,
        #[max_length = 255]
        mfa_secret -> Nullable<Varchar>,
        mfa_enabled -> Bool,
        mfa_backup_codes -> Nullable<Jsonb>,
        signature -> Nullable<Text>,
        dashboard_layout -> Nullable<Jsonb>,
        feature_flag_overrides -> Jsonb,
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
    }
}

diesel::joinable!(active_sessions -> users (user_uuid));
diesel::joinable!(article_content_revisions -> article_contents (article_content_id));
diesel::joinable!(article_contents -> tickets (ticket_id));
diesel::joinable!(assignment_log -> assignment_rules (rule_id));
diesel::joinable!(assignment_log -> tickets (ticket_id));
diesel::joinable!(assignment_rule_state -> assignment_rules (rule_id));
diesel::joinable!(assignment_rule_state -> users (last_assigned_user_uuid));
diesel::joinable!(assignment_rules -> groups (target_group_id));
diesel::joinable!(assignment_rules -> ticket_categories (category_id));
diesel::joinable!(attachments -> comments (comment_id));
diesel::joinable!(attachments -> users (uploaded_by));
diesel::joinable!(backup_jobs -> users (created_by));
diesel::joinable!(canned_responses -> users (created_by));
diesel::joinable!(category_group_visibility -> groups (group_id));
diesel::joinable!(category_group_visibility -> ticket_categories (category_id));
diesel::joinable!(category_group_visibility -> users (created_by));
diesel::joinable!(channel_credentials -> channels (channel_id));
diesel::joinable!(channel_messages -> channels (channel_id));
diesel::joinable!(channel_messages -> comments (comment_id));
diesel::joinable!(channel_messages -> tickets (ticket_id));
diesel::joinable!(channel_messages -> users (author_user_uuid));
diesel::joinable!(comments -> tickets (ticket_id));
diesel::joinable!(comments -> users (user_uuid));
diesel::joinable!(cycle_tickets -> cycles (cycle_id));
diesel::joinable!(cycle_tickets -> tickets (ticket_id));
diesel::joinable!(cycle_tickets -> users (added_by));
diesel::joinable!(cycles -> projects (project_id));
diesel::joinable!(cycles -> users (created_by));
diesel::joinable!(device_groups -> devices (device_id));
diesel::joinable!(device_groups -> groups (group_id));
diesel::joinable!(device_groups -> users (created_by));
diesel::joinable!(documentation_collection_pages -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_collection_pages -> users (created_by));
diesel::joinable!(documentation_collection_visibility -> documentation_collections (collection_id));
diesel::joinable!(documentation_collection_visibility -> groups (group_id));
diesel::joinable!(documentation_collections -> users (created_by));
diesel::joinable!(documentation_page_tickets -> documentation_pages (page_id));
diesel::joinable!(documentation_page_tickets -> tickets (ticket_id));
diesel::joinable!(documentation_page_tickets -> users (created_by));
diesel::joinable!(documentation_page_visibility -> documentation_pages (page_id));
diesel::joinable!(documentation_page_visibility -> groups (group_id));
diesel::joinable!(documentation_revisions -> documentation_pages (page_id));
diesel::joinable!(documentation_revisions -> users (created_by));
diesel::joinable!(documentation_starred_pages -> documentation_pages (page_id));
diesel::joinable!(documentation_starred_pages -> users (user_uuid));
diesel::joinable!(documentation_subscriptions -> documentation_pages (page_id));
diesel::joinable!(documentation_subscriptions -> users (user_uuid));
diesel::joinable!(group_includes -> users (created_by));
diesel::joinable!(groups -> users (created_by));
diesel::joinable!(knowledge_gap_signals -> knowledge_gaps (gap_id));
diesel::joinable!(knowledge_gaps -> documentation_pages (resolved_page_id));
diesel::joinable!(linked_tickets -> users (created_by));
diesel::joinable!(notification_preferences -> notification_types (notification_type_id));
diesel::joinable!(notification_preferences -> users (user_uuid));
diesel::joinable!(notification_rate_limits -> notification_types (notification_type_id));
diesel::joinable!(notification_rate_limits -> users (user_uuid));
diesel::joinable!(notifications -> notification_types (notification_type_id));
diesel::joinable!(notifications -> users (user_uuid));
diesel::joinable!(passkey_credentials -> users (user_uuid));
diesel::joinable!(plugin_activity -> plugins (plugin_id));
diesel::joinable!(plugin_activity -> users (user_uuid));
diesel::joinable!(plugin_collection_rows -> plugin_collection_schemas (schema_id));
diesel::joinable!(plugin_collection_rows -> plugins (plugin_id));
diesel::joinable!(plugin_collection_rows -> users (created_by));
diesel::joinable!(plugin_collection_schemas -> plugins (plugin_id));
diesel::joinable!(plugin_data -> plugins (plugin_id));
diesel::joinable!(plugins -> users (installed_by));
diesel::joinable!(project_tickets -> projects (project_id));
diesel::joinable!(project_tickets -> tickets (ticket_id));
diesel::joinable!(project_tickets -> users (created_by));
diesel::joinable!(refresh_tokens -> users (user_uuid));
diesel::joinable!(reset_tokens -> users (user_uuid));
diesel::joinable!(saved_views -> users (created_by));
diesel::joinable!(security_events -> active_sessions (session_id));
diesel::joinable!(security_events -> users (user_uuid));
diesel::joinable!(site_settings -> users (updated_by));
diesel::joinable!(sync_history -> users (initiated_by));
diesel::joinable!(ticket_categories -> users (created_by));
diesel::joinable!(ticket_devices -> devices (device_id));
diesel::joinable!(ticket_devices -> tickets (ticket_id));
diesel::joinable!(ticket_devices -> users (created_by));
diesel::joinable!(tickets -> channels (origin_channel_id));
diesel::joinable!(tickets -> ticket_categories (category_id));
diesel::joinable!(tickets -> workflow_states (workflow_state_id));
diesel::joinable!(user_groups -> groups (group_id));
diesel::joinable!(user_ticket_views -> tickets (ticket_id));
diesel::joinable!(user_ticket_views -> users (user_uuid));
diesel::joinable!(webhook_deliveries -> webhooks (webhook_id));
diesel::joinable!(webhooks -> users (created_by));
diesel::joinable!(workflow_states -> users (created_by));

diesel::allow_tables_to_appear_in_same_query!(
    active_sessions,api_tokens,article_content_revisions,article_contents,assignment_log,assignment_rule_state,assignment_rules,attachments,audit_log,backup_jobs,canned_responses,category_group_visibility,channel_credentials,channel_messages,channels,comments,cycle_tickets,cycles,device_groups,devices,documentation_collection_pages,documentation_collection_visibility,documentation_collections,documentation_page_embeddings,documentation_page_tickets,documentation_page_visibility,documentation_pages,documentation_revisions,documentation_starred_pages,documentation_subscriptions,group_includes,groups,knowledge_gap_signals,knowledge_gaps,linked_tickets,notification_preferences,notification_rate_limits,notification_types,notifications,passkey_credentials,plugin_activity,plugin_collection_rows,plugin_collection_schemas,plugin_data,plugin_local_signing_key,plugin_registry_state,plugin_trusted_publishers,plugins,project_tickets,projects,refresh_tokens,reset_tokens,saved_views,search_index_state,search_query_log,security_events,site_settings,sla_policies,sync_actions,sync_delta_tokens,sync_history,system_meta,ticket_categories,ticket_devices,tickets,user_auth_identities,user_emails,user_groups,user_ticket_views,users,webhook_deliveries,webhooks,workflow_states,working_calendar_holidays,working_calendars,);
