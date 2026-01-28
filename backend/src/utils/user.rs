use uuid::Uuid;
use crate::models::{NewUser, UserRole};

/// Builder for creating NewUser instances with sensible defaults
/// Email is stored separately and returned in build_with_email()
/// Password is NOT stored in User anymore - it goes in user_auth_identities table
pub struct NewUserBuilder {
    uuid: Uuid,
    name: String,
    email: String, // Stored but not part of NewUser - returned separately
    role: UserRole,
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
            uuid: Uuid::now_v7(),
            name,
            email,
            role,
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
    /// Password must be handled separately in user_auth_identities table
    pub fn build_with_email(self) -> (NewUser, String) {
        let new_user = NewUser {
            uuid: self.uuid,
            name: self.name,
            role: self.role,
            pronouns: self.pronouns,
            avatar_url: self.avatar_url,
            banner_url: self.banner_url,
            avatar_thumb: self.avatar_thumb,
            theme: None, // Default to system theme (handled by database default)
            microsoft_uuid: self.microsoft_uuid,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        };
        (new_user, self.email)
    }

    /// Build NewUser only (for cases where email is handled separately)
    /// Password must be handled separately in user_auth_identities table
    pub fn build(self) -> NewUser {
        NewUser {
            uuid: self.uuid,
            name: self.name,
            role: self.role,
            pronouns: self.pronouns,
            avatar_url: self.avatar_url,
            banner_url: self.banner_url,
            avatar_thumb: self.avatar_thumb,
            theme: None, // Default to system theme (handled by database default)
            microsoft_uuid: self.microsoft_uuid,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        }
    }
}

/// Convenience functions for common user creation patterns
/// Note: Password must be handled separately in user_auth_identities table
impl NewUserBuilder {
    pub fn local_user(name: String, email: String, role: UserRole) -> Self {
        Self::new(name, email, role)
    }

    pub fn oauth_user(name: String, email: String, role: UserRole) -> Self {
        Self::new(name, email, role)
    }

    pub fn microsoft_user(name: String, email: String, role: UserRole, microsoft_uuid: Option<Uuid>) -> Self {
        Self::new(name, email, role).with_microsoft_uuid(microsoft_uuid)
    }

    pub fn admin_user(name: String, email: String) -> Self {
        Self::new(name, email, UserRole::Admin)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_role() {
        let user = NewUserBuilder::new("Alice".into(), "alice@example.com".into(), UserRole::Admin).build();
        assert_eq!(user.role, UserRole::Admin);
        assert_eq!(user.name, "Alice");
    }

    #[test]
    fn builder_defaults_mfa_disabled() {
        let user = NewUserBuilder::new("Bob".into(), "b@b.com".into(), UserRole::User).build();
        assert!(!user.mfa_enabled);
        assert!(user.mfa_secret.is_none());
        assert!(user.mfa_backup_codes.is_none());
    }

    #[test]
    fn build_with_email_returns_email_separately() {
        let (user, email) = NewUserBuilder::new("Carol".into(), "carol@x.com".into(), UserRole::Technician)
            .build_with_email();
        assert_eq!(email, "carol@x.com");
        assert_eq!(user.name, "Carol");
    }

    #[test]
    fn admin_factory_sets_admin_role() {
        let user = NewUserBuilder::admin_user("Admin".into(), "a@a.com".into()).build();
        assert_eq!(user.role, UserRole::Admin);
    }

    #[test]
    fn microsoft_factory_sets_microsoft_uuid() {
        let ms_uuid = Uuid::new_v4();
        let user = NewUserBuilder::microsoft_user("MS".into(), "ms@x.com".into(), UserRole::User, Some(ms_uuid)).build();
        assert_eq!(user.microsoft_uuid, Some(ms_uuid));
    }

    #[test]
    fn builder_with_methods_override_defaults() {
        let user = NewUserBuilder::new("D".into(), "d@d.com".into(), UserRole::User)
            .with_pronouns(Some("they/them".into()))
            .with_avatar(Some("/avatar.png".into()), Some("/thumb.png".into()))
            .with_banner(Some("/banner.png".into()))
            .build();
        assert_eq!(user.pronouns, Some("they/them".into()));
        assert_eq!(user.avatar_url, Some("/avatar.png".into()));
        assert_eq!(user.avatar_thumb, Some("/thumb.png".into()));
        assert_eq!(user.banner_url, Some("/banner.png".into()));
    }

    #[test]
    fn normalize_user_data_trims_and_lowercases_email() {
        let (name, email) = normalization::normalize_user_data("  Alice  ", "  Alice@Example.COM  ");
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@example.com");
    }

    #[test]
    fn normalize_optional_string_handles_none() {
        assert_eq!(normalization::normalize_optional_string(None), None);
    }

    #[test]
    fn normalize_optional_string_trims() {
        let input = "  hello  ".to_string();
        assert_eq!(normalization::normalize_optional_string(Some(&input)), Some("hello".to_string()));
    }
}
