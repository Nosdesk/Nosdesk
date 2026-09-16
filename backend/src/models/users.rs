use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =====================================================================
// Phase 4 W2: split-role model. PlatformRole + WorkspaceRole sit
// alongside the legacy UserRole during the sweep, then UserRole is
// deleted in the cleanup migration.
// =====================================================================

/// Platform-wide privilege role. Replaces the global `users.role`
/// for non-workspace gating. Only two values: `platform_admin`
/// (super-user across the instance) and `user` (default, no
/// platform privileges). Stored on `users.platform_role`
/// (VARCHAR(32)) — string at the DB layer, enum at the application
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    PlatformAdmin,
    /// Read-only access to the instance-wide audit surface. Holds no
    /// write access to any business entity and no admin-panel access
    /// beyond the audit view. Replaces the legacy
    /// `"audit_reviewer"`; audit reads are still additionally
    /// gated on the `audit:read` token scope.
    AuditReviewer,
    User,
}

impl PlatformRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlatformAdmin => "platform_admin",
            Self::AuditReviewer => "audit_reviewer",
            Self::User => "user",
        }
    }

    /// Parse a database string into a `PlatformRole`. Unknown values
    /// fall back to `User` — defensive against legacy rows or a
    /// hand-edited CHECK that hasn't caught up. The CHECK constraint
    /// makes this branch unreachable in practice but the caller still
    /// gets a typed value either way.
    pub fn from_db(s: &str) -> Self {
        match s {
            "platform_admin" => Self::PlatformAdmin,
            "audit_reviewer" => Self::AuditReviewer,
            _ => Self::User,
        }
    }

    pub fn is_platform_admin(&self) -> bool {
        matches!(self, Self::PlatformAdmin)
    }

    /// True for principals allowed to read the instance audit surface:
    /// platform admins and the dedicated audit reviewer. This is the
    /// role half of the audit gate; callers still AND it with the
    /// `audit:read` token scope.
    pub fn can_read_audit(&self) -> bool {
        matches!(self, Self::PlatformAdmin | Self::AuditReviewer)
    }
}

/// Per-workspace privilege role. Stored on `workspace_members.role`
/// (VARCHAR(32) CHECK IN ('owner', 'admin', 'agent', 'member')).
/// The ordering implements escalation: `Owner > Admin > Agent > Member`,
/// so `require_workspace_role(Agent)` admits owners and admins as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    /// File tickets, read docs. Default for new workspace members.
    Member,
    /// Handle tickets (was global `technician` in the pre-W2 model).
    Agent,
    /// Manage workspace members + settings.
    Admin,
    /// One per workspace. Can delete the workspace.
    Owner,
}

impl WorkspaceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Agent => "agent",
            Self::Member => "member",
        }
    }

    /// Parse a database string into a `WorkspaceRole`. Unknown
    /// values fall back to `Member` — same defensive shape as
    /// [`PlatformRole::from_db`].
    pub fn from_db(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            "agent" => Self::Agent,
            _ => Self::Member,
        }
    }

    /// True if `self` meets or exceeds `min` per the role ordering
    /// (Owner > Admin > Agent > Member).
    pub fn meets(&self, min: WorkspaceRole) -> bool {
        *self >= min
    }

    /// True for staff roles (Owner / Admin / Agent) — the seats that count
    /// toward a workspace's `seat_limit`. End-user `Member` (ticket
    /// requesters) are uncapped.
    pub fn is_staff(&self) -> bool {
        self.meets(WorkspaceRole::Agent)
    }
}

// User model - updated to match the actual database schema
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::users)]
#[diesel(primary_key(uuid))]
pub struct User {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - now stored in user_emails table only
    // `role` (UserRole) was the pre-W2 column. W2 split it into
    // `platform_role` (kept below) and per-workspace
    // `workspace_members.role`. The legacy projection lives on
    // `AuthContext::role` (derived) for handler code that branches
    // on staff vs end-user.
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub password_changed_at: Option<NaiveDateTime>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_enabled: bool,
    // Recovery codes moved to the dedicated `user_recovery_codes`
    // table (migration `2026-05-31-180000_decouple_user_recovery_codes`).
    // Use `repository::user_recovery_codes` for reads / writes; this
    // table no longer carries them.
    /// Per-user feature flag overrides merged on top of the
    /// workspace defaults at request time. Same JSONB shape as
    /// `site_settings.feature_flags`. Used to opt individuals into
    /// staged rollouts before flipping the workspace default.
    /// Stays on `users` (not in `user_preferences`) because it's
    /// an admin-set override, not a user-chosen preference.
    pub feature_flag_overrides: serde_json::Value,
    /// Set when an admin soft-deletes the user. Non-null rows are
    /// hidden from "find an active user" code paths (login, mention
    /// search, assignee pickers, the default paginated list) and
    /// scheduled for purge by the retention worker once
    /// `deleted_at + NOSDESK_USER_PURGE_GRACE_DAYS` has elapsed.
    /// Historical references (audit log, ticket history) keep
    /// rendering the user so the record stays coherent during the
    /// window.
    pub deleted_at: Option<NaiveDateTime>,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `uuid.as_bytes()` so a row swap fails the tag check.
    /// `mfa_secret_kek_id` mirrors the version encoded in the blob;
    /// they MUST agree on read or the row is rejected.
    pub mfa_secret: Option<Vec<u8>>,
    pub mfa_secret_kek_id: Option<i16>,
    /// Platform-wide privilege role (Phase 4 W2). Values:
    /// `"platform_admin"` (super-user — workspace lifecycle,
    /// instance settings, hosted billing) or `"user"` (default).
    /// Read via [`PlatformRole::from_str`] for typed access. Will
    /// supersede [`UserRole`] for non-workspace privilege gating
    /// once the post-W2 sweep removes the legacy column.
    pub platform_role: String,
    /// Global username handle, projected from the control-plane IdP
    /// (orchestration O4). `None` until the user claims one. The CP
    /// is authoritative + validates it; the product only stores and
    /// (later) surfaces it (e.g. `@handle` mentions). Last field to
    /// match the appended schema column.
    pub username: Option<String>,
}

// New user for creation
// Note: Email is no longer part of NewUser - it's created separately in user_emails table
#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct NewUser {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - handled separately via user_emails table
    // `role` removed by the W2 column drop. Callers thread the
    // intended `UserRole` through `create_user_with_email` as a
    // separate parameter and that helper derives platform_role +
    // seeds the workspace_members row from it.
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_secret: Option<Vec<u8>>,
    pub mfa_secret_kek_id: Option<i16>,
    pub mfa_enabled: bool,
    /// Phase 4 W2: platform-wide privilege role. `None` leaves the
    /// DB default (`'user'`) so existing callers don't need to
    /// thread the value through. W2-aware callers set
    /// `Some("platform_admin".into())` for the bootstrap / hosted
    /// signup paths.
    pub platform_role: Option<String>,
    // mfa_backup_codes lives in `user_recovery_codes` now.
}

// User update struct
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserUpdate {
    pub name: Option<String>,
    // Email removed - update via user_emails table instead
    // `role` dropped with the column; admin role changes now go
    // through workspace_members.role (admin_workspaces handlers).
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

// User update with password for admin/user management
#[derive(Debug, Serialize, Deserialize)]
pub struct UserUpdateWithPassword {
    pub name: Option<String>,
    // Email removed - update via user_emails table
    pub role: Option<String>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub password: Option<String>,
    /// Free-form text appended to outbound channel replies as the
    /// agent's signature. `None` in the payload → no change. Empty
    /// string clears it.
    pub signature: Option<String>,
    /// Dashboard layout JSON (see `UserUpdate::dashboard_layout`).
    #[serde(default)]
    pub dashboard_layout: Option<serde_json::Value>,
    /// BCP-47 locale preference. `None` in the payload = no change;
    /// empty string = clear back to "inherit site default".
    #[serde(default)]
    pub locale: Option<String>,
    /// IANA timezone preference. Same omission / empty-string
    /// semantics as `locale`.
    #[serde(default)]
    pub timezone: Option<String>,
}

// User response with minimal information.
//
// `theme` / `dashboard_layout` / `signature` / `locale` /
// `timezone` are flattened in from the `user_preferences` row by
// `repository::user_helpers::get_user_with_primary_email` so the
// API shape stays stable for the frontend even though these
// fields now live in a separate table.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub uuid: Uuid,
    pub name: String,
    pub email: Option<String>, // Now optional - populated from user_emails table
    /// Platform-wide privilege role (platform_admin / audit_reviewer /
    /// user). Replaces the legacy derived `role`.
    pub platform_role: PlatformRole,
    /// The user's role in the workspace this response was built for
    /// (owner / admin / agent / member), or null when there's no
    /// membership / the response wasn't built with a workspace
    /// connection. The `From<User>` conversion (no DB access) always
    /// leaves this null; the populated builders fill it from
    /// `workspace_members`.
    pub workspace_role: Option<WorkspaceRole>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ticket_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_count: Option<i64>,
    /// Per-user dashboard layout JSON, or null = client uses defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_layout: Option<serde_json::Value>,
    /// Free-form email signature appended to outbound replies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// BCP-47 locale (e.g. en-US). None means "inherit from
    /// site default". Frontend reads this on app boot to decide
    /// the i18n bundle to load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// IANA timezone (e.g. Europe/Berlin). None means "inherit
    /// from site default".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Resolved locale after walking user pref -> site default ->
    /// hardcoded fallback. Populated only by /auth/me; admin user
    /// listings leave it None to avoid the extra site_settings
    /// fetch per row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_locale: Option<String>,
    /// Resolved timezone after the same fallback chain. Same /me-
    /// only population rule as `effective_locale`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_timezone: Option<String>,
}

// ============================================================================
// User preferences — split from `users` in 2026-05-14 once the
// preference set grew past the few-columns-on-the-main-table
// threshold. A row exists for every user (auto-created by trigger).
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::user_preferences)]
#[diesel(primary_key(user_uuid))]
pub struct UserPreferences {
    pub user_uuid: Uuid,
    pub theme: Option<String>,
    pub signature: Option<String>,
    pub dashboard_layout: Option<serde_json::Value>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// When true, only human-originated notifications interrupt (toast /
    /// desktop); system/automation-triggered ones land quietly in the bell.
    pub interrupt_human_only: bool,
}

// ============================================================================
// User contact fields: per-workspace custom-field schema + per-user profile
// ============================================================================

/// A per-(user × workspace) contact record: SCIM-Enterprise standard columns
/// + the custom-field values. `directory_synced` marks the standard columns as
/// Graph-owned (read-only) for that user.
#[derive(Debug, Serialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::user_profiles)]
#[diesel(primary_key(workspace_id, user_uuid))]
pub struct UserProfile {
    pub user_uuid: Uuid,
    pub workspace_id: i32,
    pub job_title: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    pub custom_fields: serde_json::Value,
    pub directory_synced: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
    /// Per-workspace display-name override (O7). `None` → render the
    /// global `users.name`.
    pub display_name: Option<String>,
    /// Per-workspace avatar override. `None` → render the global
    /// `users.avatar_url`. Product-owned, so it stays clear of the control
    /// plane's re-projection of the global avatar. Last field to match the
    /// appended column.
    pub avatar_url: Option<String>,
}

/// Insert form for a profile row (workspace_id defaults from the GUC).
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::user_profiles)]
pub struct NewUserProfile {
    pub user_uuid: Uuid,
    pub job_title: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    pub custom_fields: serde_json::Value,
    pub directory_synced: bool,
    pub created_by: Option<Uuid>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Editable profile fields from the user-side (manual surface; directory_synced
/// standard cols are rejected when sync-owned). `custom_fields` is validated
/// against the workspace schema before write.
#[derive(Debug, Deserialize)]
pub struct UserProfileInput {
    pub job_title: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    #[serde(default)]
    pub custom_fields: serde_json::Value,
    /// Per-workspace display-name override (O7). Absent/`None` clears it, so
    /// the workspace renders the user's global (control-plane) name. Same
    /// full-replace semantics as the other standard fields on this PUT.
    pub display_name: Option<String>,
    /// Per-workspace avatar override. Absent/`None` clears it, so the workspace
    /// renders the user's global avatar. Full-replace, like the other fields.
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// Partial update payload. Each field uses the `Option<Option<T>>`
/// convention so the API can distinguish "leave as-is" (outer
/// None) from "clear back to site default / role default"
/// (Some(None)) from "set to this value" (Some(Some(_))).
#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_preferences)]
pub struct UpdateUserPreferences {
    pub theme: Option<Option<String>>,
    pub signature: Option<Option<String>>,
    pub dashboard_layout: Option<Option<serde_json::Value>>,
    pub locale: Option<Option<String>>,
    pub timezone: Option<Option<String>>,
}

// User info for comments - minimal user data to include with comments
#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub uuid: Uuid,
    pub name: String,
}

// Enhanced UserInfo with avatar data for efficient frontend display
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfoWithAvatar {
    pub uuid: Uuid,
    pub name: String,
    pub avatar_url: Option<String>,
    pub avatar_thumb: Option<String>,
}

// Convert a bare User to UserResponse.
//
// Email + preference fields (theme, signature, dashboard_layout,
// locale, timezone) all come from other tables; this From impl
// leaves them None. Callers that need the fully-populated shape
// use `repository::user_helpers::get_user_with_primary_email`,
// which does the joins and fills them in.
impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        // The conversion has no DB access, so it can only surface the
        // platform role (which lives on the User row). The
        // workspace_role is left None; callers that need it should
        // build UserResponse via
        // `repository::user_helpers::get_user_with_primary_email`
        // (which has a connection and looks up workspace_members).
        UserResponse {
            uuid: user.uuid,
            name: user.name,
            email: None,
            platform_role: PlatformRole::from_db(&user.platform_role),
            workspace_role: None,
            pronouns: user.pronouns,
            avatar_url: user.avatar_url,
            banner_url: user.banner_url,
            avatar_thumb: user.avatar_thumb,
            theme: None,
            microsoft_uuid: user.microsoft_uuid,
            created_at: user.created_at,
            updated_at: user.updated_at,
            open_ticket_count: None,
            device_count: None,
            dashboard_layout: None,
            signature: None,
            locale: None,
            timezone: None,
            effective_locale: None,
            effective_timezone: None,
        }
    }
}

// Convert User to UserInfo
impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        UserInfo {
            uuid: user.uuid,
            name: user.name,
        }
    }
}

impl From<User> for UserInfoWithAvatar {
    fn from(user: User) -> Self {
        UserInfoWithAvatar {
            uuid: user.uuid,
            name: user.name,
            avatar_url: user.avatar_url,
            avatar_thumb: user.avatar_thumb,
        }
    }
}

// Borrowing variant so list builders can project straight from a
// `&User` held in a batch lookup map, without cloning the whole row.
impl From<&User> for UserInfoWithAvatar {
    fn from(user: &User) -> Self {
        UserInfoWithAvatar {
            uuid: user.uuid,
            name: user.name.clone(),
            avatar_url: user.avatar_url.clone(),
            avatar_thumb: user.avatar_thumb.clone(),
        }
    }
}

// User Email models for storing multiple email addresses per user
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::user_emails)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
pub struct UserEmail {
    pub id: i32,
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct NewUserEmail {
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct UserEmailUpdate {
    pub is_primary: Option<bool>,
    /// No `is_verified`. Verification is a claim about the address, provable
    /// only by a challenge sent to it, so there is no request whose body may
    /// carry it. Removing the field means a future handler cannot reintroduce
    /// the hole by passing one through; it has to add a writer deliberately.
    pub updated_at: Option<NaiveDateTime>,
}

// Extended User response that includes all email addresses
#[derive(Debug, Serialize, Deserialize)]
pub struct UserWithEmails {
    #[serde(flatten)]
    pub user: UserResponse,
    pub emails: Vec<UserEmail>,
}
