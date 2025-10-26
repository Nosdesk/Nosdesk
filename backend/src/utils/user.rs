use uuid::Uuid;
use crate::models::{NewUser, UserRole};

/// Builder for creating NewUser instances with sensible defaults
/// Email is stored separately and returned in build_with_email()
pub struct NewUserBuilder {
    uuid: Uuid,
    name: String,
    email: String, // Stored but not part of NewUser - returned separately
    role: UserRole,
    password_hash: Vec<u8>,
    pronouns: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    avatar_thumb: Option<String>,
    microsoft_uuid: Option<Uuid>,
}

impl NewUserBuilder {
    /// Create a new user builder with required fields
    pub fn new(name: String, email: String, role: UserRole) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            name,
            email,
            role,
            password_hash: Vec::new(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
        }
    }

    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = uuid;
        self
    }

    pub fn with_password_hash(mut self, hash: Vec<u8>) -> Self {
        self.password_hash = hash;
        self
    }

    pub fn with_pronouns(mut self, pronouns: Option<String>) -> Self {
        self.pronouns = pronouns;
        self
    }

    pub fn with_avatar(mut self, avatar_url: Option<String>, avatar_thumb: Option<String>) -> Self {
        self.avatar_url = avatar_url;
        self.avatar_thumb = avatar_thumb;
        self
    }

    pub fn with_banner(mut self, banner_url: Option<String>) -> Self {
        self.banner_url = banner_url;
        self
    }

    pub fn with_microsoft_uuid(mut self, microsoft_uuid: Option<Uuid>) -> Self {
        self.microsoft_uuid = microsoft_uuid;
        self
    }

    /// Build and return (NewUser, email) tuple
    /// Email is returned separately since it goes in user_emails table
    pub fn build_with_email(self) -> (NewUser, String) {
        let new_user = NewUser {
            uuid: self.uuid,
            name: self.name,
            role: self.role,
            password_hash: self.password_hash,
            pronouns: self.pronouns,
            avatar_url: self.avatar_url,
            banner_url: self.banner_url,
            avatar_thumb: self.avatar_thumb,
            microsoft_uuid: self.microsoft_uuid,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        };
        (new_user, self.email)
    }

    /// Build NewUser only (for cases where email is handled separately)
    pub fn build(self) -> NewUser {
        NewUser {
            uuid: self.uuid,
            name: self.name,
            role: self.role,
            password_hash: self.password_hash,
            pronouns: self.pronouns,
            avatar_url: self.avatar_url,
            banner_url: self.banner_url,
            avatar_thumb: self.avatar_thumb,
            microsoft_uuid: self.microsoft_uuid,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        }
    }
}

/// Convenience functions for common user creation patterns
impl NewUserBuilder {
    pub fn local_user(name: String, email: String, role: UserRole, password_hash: Vec<u8>) -> Self {
        Self::new(name, email, role).with_password_hash(password_hash)
    }

    pub fn oauth_user(name: String, email: String, role: UserRole) -> Self {
        Self::new(name, email, role)
    }

    pub fn microsoft_user(name: String, email: String, role: UserRole, microsoft_uuid: Option<Uuid>) -> Self {
        Self::new(name, email, role).with_microsoft_uuid(microsoft_uuid)
    }

    pub fn admin_user(name: String, email: String, password_hash: Vec<u8>) -> Self {
        Self::new(name, email, UserRole::Admin).with_password_hash(password_hash)
    }
}

/// Helper functions for email and name normalization
pub mod normalization {
    use crate::utils;

    pub fn normalize_user_data(name: &str, email: &str) -> (String, String) {
        (
            utils::normalize_string(name),
            utils::normalize_email(email),
        )
    }

    pub fn normalize_optional_string(value: Option<&String>) -> Option<String> {
        value.map(|s| utils::normalize_string(s))
    }
}
