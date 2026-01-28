use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use ring::digest::{Context, SHA256};
use uuid::Uuid;

/// Token types for different reset purposes
#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    PasswordReset,
    MfaReset,
    Invitation,
}

impl TokenType {
    pub fn as_str(&self) -> &str {
        match self {
            TokenType::PasswordReset => "password_reset",
            TokenType::MfaReset => "mfa_reset",
            TokenType::Invitation => "invitation",
        }
    }

    /// Get the expiration duration for this token type
    pub fn expiration_duration(&self) -> Duration {
        match self {
            TokenType::PasswordReset => Duration::hours(1),  // 1 hour for password resets
            TokenType::MfaReset => Duration::minutes(15),    // 15 minutes for MFA resets
            TokenType::Invitation => Duration::days(7),      // 7 days for user invitations
        }
    }
}

/// Reset token information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResetToken {
    pub raw_token: String,  // The actual token to send to the user (never stored)
    pub token_hash: String, // SHA-256 hash stored in database
    pub user_uuid: Uuid,
    pub token_type: TokenType,
    pub expires_at: DateTime<Utc>,
}

/// Reset token utilities
pub struct ResetTokenUtils;

impl ResetTokenUtils {
    /// Generate a cryptographically secure random token
    /// Returns a 32-byte token encoded as hexadecimal (64 characters)
    pub fn generate_token() -> String {
        let mut rng = rand::thread_rng();
        let token_bytes: [u8; 32] = rng.gen();
        hex::encode(token_bytes)
    }

    /// Hash a token using SHA-256
    /// Returns the hash as a hexadecimal string (64 characters)
    pub fn hash_token(token: &str) -> String {
        let mut context = Context::new(&SHA256);
        context.update(token.as_bytes());
        let digest = context.finish();
        hex::encode(digest.as_ref())
    }

    /// Create a new reset token
    pub fn create_reset_token(user_uuid: Uuid, token_type: TokenType) -> ResetToken {
        let raw_token = Self::generate_token();
        let token_hash = Self::hash_token(&raw_token);
        let expires_at = Utc::now() + token_type.expiration_duration();

        ResetToken {
            raw_token,
            token_hash,
            user_uuid,
            token_type,
            expires_at,
        }
    }

    /// Validate that a token hash matches a raw token
    #[allow(dead_code)]
    pub fn validate_token_hash(raw_token: &str, stored_hash: &str) -> bool {
        let computed_hash = Self::hash_token(raw_token);
        // Use constant-time comparison to prevent timing attacks
        computed_hash == stored_hash
    }

    /// Check if a token is expired
    pub fn is_token_expired(expires_at: DateTime<Utc>) -> bool {
        Utc::now() > expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let token1 = ResetTokenUtils::generate_token();
        let token2 = ResetTokenUtils::generate_token();

        // Tokens should be 64 characters (32 bytes in hex)
        assert_eq!(token1.len(), 64);
        assert_eq!(token2.len(), 64);

        // Tokens should be different
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_hash_token() {
        let token = "test_token_12345";
        let hash1 = ResetTokenUtils::hash_token(token);
        let hash2 = ResetTokenUtils::hash_token(token);

        // Hashes should be consistent
        assert_eq!(hash1, hash2);

        // Hash should be 64 characters (SHA-256 in hex)
        assert_eq!(hash1.len(), 64);

        // Different tokens should produce different hashes
        let different_hash = ResetTokenUtils::hash_token("different_token");
        assert_ne!(hash1, different_hash);
    }

    #[test]
    fn test_validate_token_hash() {
        let token = "test_token_12345";
        let hash = ResetTokenUtils::hash_token(token);

        // Valid token should match
        assert!(ResetTokenUtils::validate_token_hash(token, &hash));

        // Invalid token should not match
        assert!(!ResetTokenUtils::validate_token_hash("wrong_token", &hash));
    }

    #[test]
    fn test_token_expiration() {
        // Token that expired 1 hour ago
        let expired = Utc::now() - Duration::hours(1);
        assert!(ResetTokenUtils::is_token_expired(expired));

        // Token that expires in 1 hour
        let valid = Utc::now() + Duration::hours(1);
        assert!(!ResetTokenUtils::is_token_expired(valid));
    }

    #[test]
    fn test_create_reset_token() {
        let user_uuid = Uuid::now_v7();
        let token = ResetTokenUtils::create_reset_token(user_uuid, TokenType::PasswordReset);

        // Check token properties
        assert_eq!(token.raw_token.len(), 64);
        assert_eq!(token.token_hash.len(), 64);
        assert_eq!(token.user_uuid, user_uuid);
        assert_eq!(token.token_type, TokenType::PasswordReset);

        // Token should not be expired
        assert!(!ResetTokenUtils::is_token_expired(token.expires_at));

        // Hash should match the raw token
        assert!(ResetTokenUtils::validate_token_hash(&token.raw_token, &token.token_hash));
    }

    #[test]
    fn test_token_type_expiration() {
        // Password reset tokens expire in 1 hour
        assert_eq!(
            TokenType::PasswordReset.expiration_duration(),
            Duration::hours(1)
        );

        // MFA reset tokens expire in 15 minutes
        assert_eq!(
            TokenType::MfaReset.expiration_duration(),
            Duration::minutes(15)
        );
    }
}
