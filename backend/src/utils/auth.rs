use bcrypt::{hash, DEFAULT_COST};
use uuid::Uuid;
use crate::models::{NewUserAuthIdentity, UserAuthIdentity};
use crate::utils::{ValidationError, ValidationResult};

/// Hash a password securely using bcrypt
pub fn hash_password(password: &str) -> ValidationResult<String> {
    hash(password, DEFAULT_COST)
        .map_err(|_| ValidationError::ValidationFailed("Failed to hash password".to_string()))
}

/// Create a local authentication identity for a user
pub fn create_local_auth_identity(
    user_id: i32,
    password_hash: String,
    user_uuid: Uuid,
) -> NewUserAuthIdentity {
    NewUserAuthIdentity {
        user_uuid,
        provider_type: "local".to_string(),
        external_id: user_uuid.to_string(), // Use UUID as external ID for local auth
        email: None, // Will be set by caller if needed
        metadata: None,
        password_hash: Some(password_hash),
    }
}

/// Update an existing authentication identity with new password
pub fn update_auth_identity_password(
    existing_identity: &UserAuthIdentity,
    new_password_hash: String,
) -> NewUserAuthIdentity {
    NewUserAuthIdentity {
        user_uuid: existing_identity.user_uuid,
        provider_type: existing_identity.provider_type.clone(),
        external_id: existing_identity.external_id.clone(),
        email: existing_identity.email.clone(),
        metadata: existing_identity.metadata.clone(),
        password_hash: Some(new_password_hash),
    }
} 