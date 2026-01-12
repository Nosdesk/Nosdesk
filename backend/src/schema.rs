// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "assignment_method"))]
    pub struct AssignmentMethod;

    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "documentation_status"))]
    pub struct DocumentationStatus;

    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_status"))]
    pub struct ProjectStatus;

    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "ticket_priority"))]
    pub struct TicketPriority;

    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "ticket_status"))]
    pub struct TicketStatus;

    #[derive(diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;
}

diesel::table! {
    active_sessions (id) {
        id -> Int4,
        #[max_length = 64]
        session_token -> Varchar,
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
    category_group_visibility (category_id, group_id) {
        category_id -> Int4,
        group_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
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
        slug -> Nullable<Varchar>,
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
        ticket_id -> Nullable<Int4>,
        display_order -> Nullable<Int4>,
        is_public -> Bool,
        is_template -> Bool,
        archived_at -> Nullable<Timestamptz>,
        yjs_state_vector -> Nullable<Bytea>,
        yjs_document -> Nullable<Bytea>,
        yjs_client_id -> Nullable<Int8>,
        has_unsaved_changes -> Bool,
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
    linked_tickets (ticket_id, linked_ticket_id) {
        ticket_id -> Int4,
        linked_ticket_id -> Int4,
        #[max_length = 50]
        link_type -> Varchar,
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
    project_tickets (project_id, ticket_id) {
        project_id -> Int4,
        ticket_id -> Int4,
        created_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
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
    use super::sql_types::TicketStatus;
    use super::sql_types::TicketPriority;

    tickets (id) {
        id -> Int4,
        #[max_length = 255]
        title -> Varchar,
        status -> TicketStatus,
        priority -> TicketPriority,
        requester_uuid -> Nullable<Uuid>,
        assignee_uuid -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Nullable<Uuid>,
        closed_at -> Nullable<Timestamptz>,
        closed_by -> Nullable<Uuid>,
        category_id -> Nullable<Int4>,
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
        passkey_credentials -> Nullable<Jsonb>,
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
diesel::joinable!(category_group_visibility -> groups (group_id));
diesel::joinable!(category_group_visibility -> ticket_categories (category_id));
diesel::joinable!(category_group_visibility -> users (created_by));
diesel::joinable!(comments -> tickets (ticket_id));
diesel::joinable!(comments -> users (user_uuid));
diesel::joinable!(device_groups -> devices (device_id));
diesel::joinable!(device_groups -> groups (group_id));
diesel::joinable!(device_groups -> users (created_by));
diesel::joinable!(documentation_pages -> tickets (ticket_id));
diesel::joinable!(documentation_revisions -> documentation_pages (page_id));
diesel::joinable!(documentation_revisions -> users (created_by));
diesel::joinable!(groups -> users (created_by));
diesel::joinable!(linked_tickets -> users (created_by));
diesel::joinable!(notification_preferences -> notification_types (notification_type_id));
diesel::joinable!(notification_preferences -> users (user_uuid));
diesel::joinable!(notification_rate_limits -> notification_types (notification_type_id));
diesel::joinable!(notification_rate_limits -> users (user_uuid));
diesel::joinable!(notifications -> notification_types (notification_type_id));
diesel::joinable!(notifications -> users (user_uuid));
diesel::joinable!(project_tickets -> projects (project_id));
diesel::joinable!(project_tickets -> tickets (ticket_id));
diesel::joinable!(project_tickets -> users (created_by));
diesel::joinable!(refresh_tokens -> users (user_uuid));
diesel::joinable!(reset_tokens -> users (user_uuid));
diesel::joinable!(security_events -> active_sessions (session_id));
diesel::joinable!(security_events -> users (user_uuid));
diesel::joinable!(site_settings -> users (updated_by));
diesel::joinable!(sync_history -> users (initiated_by));
diesel::joinable!(ticket_categories -> users (created_by));
diesel::joinable!(ticket_devices -> devices (device_id));
diesel::joinable!(ticket_devices -> tickets (ticket_id));
diesel::joinable!(ticket_devices -> users (created_by));
diesel::joinable!(tickets -> ticket_categories (category_id));
diesel::joinable!(user_emails -> users (user_uuid));
diesel::joinable!(user_groups -> groups (group_id));
diesel::joinable!(user_ticket_views -> tickets (ticket_id));
diesel::joinable!(user_ticket_views -> users (user_uuid));
diesel::joinable!(webhook_deliveries -> webhooks (webhook_id));
diesel::joinable!(webhooks -> users (created_by));

diesel::allow_tables_to_appear_in_same_query!(
    active_sessions,api_tokens,article_content_revisions,article_contents,assignment_log,assignment_rule_state,assignment_rules,attachments,backup_jobs,category_group_visibility,comments,device_groups,devices,documentation_pages,documentation_revisions,groups,linked_tickets,notification_preferences,notification_rate_limits,notification_types,notifications,project_tickets,projects,refresh_tokens,reset_tokens,security_events,site_settings,sync_delta_tokens,sync_history,ticket_categories,ticket_devices,tickets,user_auth_identities,user_emails,user_groups,user_ticket_views,users,webhook_deliveries,webhooks,);
