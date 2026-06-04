use crate::models::{NewUser, PlatformRole};
use uuid::Uuid;

/// Builder for creating NewUser instances with sensible defaults
/// Email is stored separately and returned in build_with_email()
/// Password is NOT stored in User anymore - it goes in user_auth_identities table
///
/// The builder only carries the platform role (which lives on the
/// `users` row). The per-workspace membership role is supplied
/// separately to `create_user_with_email`.
pub struct NewUserBuilder {
    uuid: Uuid,
    name: String,
    email: String, // Stored but not part of NewUser - returned separately
    platform_role: PlatformRole,
    pronouns: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    avatar_thumb: Option<String>,
    microsoft_uuid: Option<Uuid>,
}

impl NewUserBuilder {
    /// Create a new user builder with required fields
    pub fn new(name: String, email: String, platform_role: PlatformRole) -> Self {
        Self {
            uuid: Uuid::now_v7(),
            name,
            email,
            platform_role,
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

    /// Build and return (NewUser, email). The platform role is baked
    /// onto the NewUser; the workspace membership role is supplied
    /// separately to `create_user_with_email`. Email goes to
    /// user_emails. Password handled separately in
    /// user_auth_identities.
    pub fn build_with_email(self) -> (NewUser, String) {
        let email = self.email.clone();
        (self.build_new_user(), email)
    }

    /// Build a NewUser for cases where email is handled separately.
    pub fn build(self) -> NewUser {
        self.build_new_user()
    }

    fn build_new_user(self) -> NewUser {
        NewUser {
            uuid: self.uuid,
            name: self.name,
            pronouns: self.pronouns,
            avatar_url: self.avatar_url,
            banner_url: self.banner_url,
            avatar_thumb: self.avatar_thumb,
            microsoft_uuid: self.microsoft_uuid,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some(self.platform_role.as_str().to_string()),
        }
    }
}

/// Convenience functions for common user creation patterns
/// Note: Password must be handled separately in user_auth_identities table
impl NewUserBuilder {
    pub fn local_user(name: String, email: String, platform_role: PlatformRole) -> Self {
        Self::new(name, email, platform_role)
    }

    pub fn oauth_user(name: String, email: String, platform_role: PlatformRole) -> Self {
        Self::new(name, email, platform_role)
    }

    pub fn microsoft_user(
        name: String,
        email: String,
        platform_role: PlatformRole,
        microsoft_uuid: Option<Uuid>,
    ) -> Self {
        Self::new(name, email, platform_role).with_microsoft_uuid(microsoft_uuid)
    }

    pub fn admin_user(name: String, email: String) -> Self {
        Self::new(name, email, PlatformRole::PlatformAdmin)
    }
}

/// Helper functions for email and name normalization
pub mod normalization {
    use crate::utils;

    pub fn normalize_user_data(name: &str, email: &str) -> (String, String) {
        (utils::normalize_string(name), utils::normalize_email(email))
    }

    pub fn normalize_optional_string(value: Option<&str>) -> Option<String> {
        value.map(utils::normalize_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_role() {
        let user = NewUserBuilder::new(
            "Alice".into(),
            "alice@example.com".into(),
            PlatformRole::PlatformAdmin,
        )
        .build();
        assert_eq!(user.name, "Alice");
        assert_eq!(user.platform_role.as_deref(), Some("platform_admin"));
    }

    #[test]
    fn builder_defaults_mfa_disabled() {
        let user = NewUserBuilder::new("Bob".into(), "b@b.com".into(), PlatformRole::User).build();
        assert!(!user.mfa_enabled);
        assert!(user.mfa_secret.is_none());
        // Recovery codes are now their own table (user_recovery_codes);
        // the NewUserBuilder no longer carries them.
    }

    #[test]
    fn build_with_email_returns_email_separately() {
        let (user, email) =
            NewUserBuilder::new("Carol".into(), "carol@x.com".into(), PlatformRole::User)
                .build_with_email();
        assert_eq!(email, "carol@x.com");
        assert_eq!(user.name, "Carol");
    }

    #[test]
    fn admin_factory_sets_admin_role() {
        let user = NewUserBuilder::admin_user("Admin".into(), "a@a.com".into()).build();
        assert_eq!(user.platform_role.as_deref(), Some("platform_admin"));
    }

    #[test]
    fn microsoft_factory_sets_microsoft_uuid() {
        let ms_uuid = Uuid::new_v4();
        let user = NewUserBuilder::microsoft_user(
            "MS".into(),
            "ms@x.com".into(),
            PlatformRole::User,
            Some(ms_uuid),
        )
        .build();
        assert_eq!(user.microsoft_uuid, Some(ms_uuid));
    }

    #[test]
    fn builder_with_methods_override_defaults() {
        let user = NewUserBuilder::new("D".into(), "d@d.com".into(), PlatformRole::User)
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
        let (name, email) =
            normalization::normalize_user_data("  Alice  ", "  Alice@Example.COM  ");
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@example.com");
    }

    #[test]
    fn normalize_optional_string_handles_none() {
        assert_eq!(normalization::normalize_optional_string(None), None);
    }

    #[test]
    fn normalize_optional_string_trims() {
        assert_eq!(
            normalization::normalize_optional_string(Some("  hello  ")),
            Some("hello".to_string())
        );
    }
}
