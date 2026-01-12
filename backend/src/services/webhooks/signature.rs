//! Webhook Signature
//!
//! HMAC-SHA256 signature generation for webhook payloads.

use ring::hmac;

/// Generate HMAC-SHA256 signature for a payload
pub fn sign_payload(payload: &str, secret: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, payload.as_bytes());
    format!("sha256={}", hex::encode(signature.as_ref()))
}

/// Verify HMAC-SHA256 signature (constant-time comparison)
#[allow(dead_code)]
pub fn verify_signature(payload: &str, secret: &str, signature: &str) -> bool {
    let expected = sign_payload(payload, secret);
    ring::constant_time::verify_slices_are_equal(expected.as_bytes(), signature.as_bytes()).is_ok()
}

/// Generate a random secret for new webhooks
pub fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    format!("whsec_{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let payload = r#"{"event":"test"}"#;
        let secret = "whsec_test_secret_123";

        let signature = sign_payload(payload, secret);
        assert!(signature.starts_with("sha256="));
        assert!(verify_signature(payload, secret, &signature));
    }

    #[test]
    fn test_generate_secret() {
        let secret = generate_secret();
        assert!(secret.starts_with("whsec_"));
        assert_eq!(secret.len(), 70); // "whsec_" (6) + 64 hex chars
    }
}
