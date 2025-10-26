// @generated automatically by Diesel CLI.

pub mod sql_types {
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
        ip_address -> Nullable<Text>,
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
    article_contents (id) {
        id -> Int4,
        content -> Text,
        ticket_id -> Nullable<Int4>,
    }
}

diesel::table! {
    attachments (id) {
        id -> Int4,
        #[max_length = 255]
        url -> Varchar,
        #[max_length = 255]
        name -> Varchar,
        comment_id -> Nullable<Int4>,
    }
}

diesel::table! {
    comments (id) {
        id -> Int4,
        content -> Text,
        ticket_id -> Int4,
        user_id -> Int4,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
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
        notes -> Nullable<Text>,
        user_id -> Nullable<Int4>,
        primary_user_uuid -> Nullable<Uuid>,
        #[max_length = 255]
        azure_device_id -> Nullable<Varchar>,
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
        #[max_length = 500]
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
        estimated_reading_time -> Nullable<Int4>,
        word_count -> Nullable<Int4>,
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
        word_count -> Nullable<Int4>,
    }
}

diesel::table! {
    linked_tickets (ticket_id, linked_ticket_id) {
        ticket_id -> Int4,
        linked_ticket_id -> Int4,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    project_tickets (project_id, ticket_id) {
        project_id -> Int4,
        ticket_id -> Int4,
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
        ip_address -> Nullable<Text>,
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
        ip_address -> Nullable<Text>,
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
    }
}

diesel::table! {
    ticket_devices (ticket_id, device_id) {
        ticket_id -> Int4,
        device_id -> Int4,
        created_at -> Timestamptz,
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
        description -> Nullable<Text>,
        status -> TicketStatus,
        priority -> TicketPriority,
        requester_uuid -> Nullable<Uuid>,
        assignee_uuid -> Nullable<Uuid>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        closed_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    user_auth_identities (id) {
        id -> Int4,
        user_id -> Int4,
        #[max_length = 50]
        provider_type -> Varchar,
        #[max_length = 255]
        external_id -> Varchar,
        #[max_length = 255]
        email -> Nullable<Varchar>,
        metadata -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        #[max_length = 255]
        password_hash -> Nullable<Varchar>,
    }
}

diesel::table! {
    user_emails (id) {
        id -> Int4,
        user_id -> Int4,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 50]
        email_type -> Varchar,
        is_primary -> Bool,
        is_verified -> Bool,
        #[max_length = 50]
        source -> Nullable<Varchar>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
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

    users (id) {
        id -> Int4,
        uuid -> Uuid,
        #[max_length = 255]
        name -> Varchar,
        role -> UserRole,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        password_hash -> Bytea,
        password_changed_at -> Nullable<Timestamptz>,
        #[max_length = 100]
        pronouns -> Nullable<Varchar>,
        #[max_length = 500]
        avatar_url -> Nullable<Varchar>,
        #[max_length = 500]
        banner_url -> Nullable<Varchar>,
        #[max_length = 500]
        avatar_thumb -> Nullable<Varchar>,
        microsoft_uuid -> Nullable<Uuid>,
        #[max_length = 255]
        mfa_secret -> Nullable<Varchar>,
        mfa_enabled -> Bool,
        mfa_backup_codes -> Nullable<Jsonb>,
        passkey_credentials -> Nullable<Jsonb>,
    }
}

diesel::joinable!(article_contents -> tickets (ticket_id));
diesel::joinable!(attachments -> comments (comment_id));
diesel::joinable!(comments -> tickets (ticket_id));
diesel::joinable!(comments -> users (user_id));
diesel::joinable!(devices -> users (user_id));
diesel::joinable!(documentation_pages -> tickets (ticket_id));
diesel::joinable!(documentation_revisions -> documentation_pages (page_id));
diesel::joinable!(project_tickets -> projects (project_id));
diesel::joinable!(project_tickets -> tickets (ticket_id));
diesel::joinable!(security_events -> active_sessions (session_id));
diesel::joinable!(ticket_devices -> devices (device_id));
diesel::joinable!(ticket_devices -> tickets (ticket_id));
diesel::joinable!(user_auth_identities -> users (user_id));
diesel::joinable!(user_emails -> users (user_id));
diesel::joinable!(user_ticket_views -> tickets (ticket_id));

diesel::allow_tables_to_appear_in_same_query!(
    active_sessions,
    article_contents,
    attachments,
    comments,
    devices,
    documentation_pages,
    documentation_revisions,
    linked_tickets,
    project_tickets,
    projects,
    refresh_tokens,
    reset_tokens,
    security_events,
    sync_history,
    ticket_devices,
    tickets,
    user_auth_identities,
    user_emails,
    user_ticket_views,
    users,
);
