use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== SECURITY EVENTS MODELS =====

/// Security events for MFA and authentication monitoring
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct SecurityEvent {
    pub id: i32,
    /// `None` for events not tied to a known account (e.g. a failed
    /// login against an unrecognised email; see C/W2). The attempted
    /// identifier is carried in `details` for those rows.
    pub user_uuid: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
}

/// New security event for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct NewSecurityEvent {
    pub user_uuid: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
}

/// Security event types enum for type safety
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum SecurityEventType {
    #[serde(rename = "login_success")]
    LoginSuccess,
    #[serde(rename = "login_failed")]
    LoginFailed,
    #[serde(rename = "mfa_enabled")]
    MfaEnabled,
    #[serde(rename = "mfa_disabled")]
    MfaDisabled,
    #[serde(rename = "mfa_failed")]
    MfaFailed,
    #[serde(rename = "mfa_success")]
    MfaSuccess,
    #[serde(rename = "backup_codes_used")]
    BackupCodesUsed,
    #[serde(rename = "backup_codes_regenerated")]
    BackupCodesRegenerated,
    #[serde(rename = "password_changed")]
    PasswordChanged,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "account_locked")]
    AccountLocked,
    #[serde(rename = "suspicious_activity")]
    SuspiciousActivity,
}

impl SecurityEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoginSuccess => "login_success",
            Self::LoginFailed => "login_failed",
            Self::MfaEnabled => "mfa_enabled",
            Self::MfaDisabled => "mfa_disabled",
            Self::MfaFailed => "mfa_failed",
            Self::MfaSuccess => "mfa_success",
            Self::BackupCodesUsed => "backup_codes_used",
            Self::BackupCodesRegenerated => "backup_codes_regenerated",
            Self::PasswordChanged => "password_changed",
            Self::SessionRevoked => "session_revoked",
            Self::AccountLocked => "account_locked",
            Self::SuspiciousActivity => "suspicious_activity",
        }
    }
}

impl std::fmt::Display for SecurityEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "login_success" => Ok(Self::LoginSuccess),
            "login_failed" => Ok(Self::LoginFailed),
            "mfa_enabled" => Ok(Self::MfaEnabled),
            "mfa_disabled" => Ok(Self::MfaDisabled),
            "mfa_failed" => Ok(Self::MfaFailed),
            "mfa_success" => Ok(Self::MfaSuccess),
            "backup_codes_used" => Ok(Self::BackupCodesUsed),
            "backup_codes_regenerated" => Ok(Self::BackupCodesRegenerated),
            "password_changed" => Ok(Self::PasswordChanged),
            "session_revoked" => Ok(Self::SessionRevoked),
            "account_locked" => Ok(Self::AccountLocked),
            "suspicious_activity" => Ok(Self::SuspiciousActivity),
            _ => Err(format!("Invalid security event type: {s}")),
        }
    }
}

/// Security event severity enum
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecurityEventSeverity {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "critical")]
    Critical,
}

impl SecurityEventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for SecurityEventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventSeverity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("Invalid security event severity: {s}")),
        }
    }
}

/// Response model for security events in user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityEventResponse {
    pub id: i32,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub location: Option<String>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub details: Option<serde_json::Value>,
}

impl From<SecurityEvent> for SecurityEventResponse {
    fn from(event: SecurityEvent) -> Self {
        SecurityEventResponse {
            id: event.id,
            event_type: event.event_type,
            ip_address: event.ip_address.map(|ip| ip.to_string()),
            location: event.location,
            severity: event.severity,
            created_at: event.created_at,
            details: event.details,
        }
    }
}
