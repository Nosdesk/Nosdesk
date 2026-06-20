//! AWS SNS message signature verification.
//!
//! The inbound-email webhook is an unauthenticated endpoint (SNS POSTs to it
//! server-to-server), so the SNS message signature is the authentication: it
//! proves the payload came from AWS SNS and wasn't tampered with. Without it,
//! anyone who learned the endpoint URL could inject forged notifications
//! (arbitrary S3 keys to fetch, arbitrary mail to turn into tickets).
//!
//! We require **SignatureVersion 2** (RSA-SHA256); the legacy SHA1 version 1
//! is rejected. The signing certificate is fetched from the message's
//! `SigningCertURL`, which we first constrain to an `sns.<region>.amazonaws.com`
//! host so a forged message can't point us at an attacker-controlled cert.
//!
//! The verification splits into pure, unit-testable pieces (canonical-string
//! construction, cert-key extraction, RSA verify) plus the network fetch of
//! the certificate, which is cached by URL.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use once_cell::sync::Lazy;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use serde::Deserialize;
use sha2::Sha256;

#[derive(Debug)]
pub enum SnsError {
    /// Body wasn't a recognised SNS envelope.
    Malformed(String),
    /// `Type` wasn't one we handle.
    UnknownType(String),
    /// A field required for this message type's canonical string was absent.
    MissingField(&'static str),
    /// Not SignatureVersion 2; we don't accept the legacy SHA1 scheme.
    UnsupportedSignatureVersion(String),
    /// `SigningCertURL` failed the AWS-host allowlist.
    UntrustedCertUrl,
    /// Fetching the signing certificate failed.
    CertFetch(String),
    /// The certificate couldn't be parsed into an RSA public key.
    BadCertificate,
    /// `Signature` wasn't valid base64 / wasn't a usable signature.
    BadSignature,
    /// Signature did not verify against the certificate.
    VerificationFailed,
}

impl std::fmt::Display for SnsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed SNS message: {m}"),
            Self::UnknownType(t) => write!(f, "unknown SNS message type: {t}"),
            Self::MissingField(field) => write!(f, "SNS message missing field: {field}"),
            Self::UnsupportedSignatureVersion(v) => {
                write!(f, "unsupported SNS signature version: {v} (require 2)")
            }
            Self::UntrustedCertUrl => write!(f, "SNS SigningCertURL is not an AWS SNS host"),
            Self::CertFetch(e) => write!(f, "fetching SNS signing certificate failed: {e}"),
            Self::BadCertificate => write!(f, "SNS signing certificate could not be parsed"),
            Self::BadSignature => write!(f, "SNS signature is malformed"),
            Self::VerificationFailed => write!(f, "SNS signature did not verify"),
        }
    }
}
impl std::error::Error for SnsError {}

/// SNS message types we accept. `Notification` carries an SES event;
/// `SubscriptionConfirmation` is the one-time handshake when the HTTPS
/// subscription is created; `UnsubscribeConfirmation` arrives if the
/// subscription is later removed.
pub const TYPE_NOTIFICATION: &str = "Notification";
pub const TYPE_SUBSCRIPTION_CONFIRMATION: &str = "SubscriptionConfirmation";
pub const TYPE_UNSUBSCRIBE_CONFIRMATION: &str = "UnsubscribeConfirmation";

/// A parsed SNS envelope. Field presence varies by `type_`; the optional
/// fields are validated when building the canonical string.
#[derive(Debug, Clone, Deserialize)]
pub struct SnsMessage {
    #[serde(rename = "Type")]
    pub type_: String,
    #[serde(rename = "MessageId")]
    pub message_id: String,
    #[serde(rename = "TopicArn")]
    pub topic_arn: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    #[serde(rename = "SignatureVersion")]
    pub signature_version: String,
    #[serde(rename = "Signature")]
    pub signature: String,
    #[serde(rename = "SigningCertURL")]
    pub signing_cert_url: String,
    #[serde(rename = "Subject")]
    pub subject: Option<String>,
    #[serde(rename = "SubscribeURL")]
    pub subscribe_url: Option<String>,
    #[serde(rename = "Token")]
    pub token: Option<String>,
}

impl SnsMessage {
    pub fn parse(body: &[u8]) -> Result<Self, SnsError> {
        serde_json::from_slice(body).map_err(|e| SnsError::Malformed(e.to_string()))
    }

    pub fn is_notification(&self) -> bool {
        self.type_ == TYPE_NOTIFICATION
    }

    pub fn is_subscription_confirmation(&self) -> bool {
        self.type_ == TYPE_SUBSCRIPTION_CONFIRMATION
    }
}

fn push_kv(buf: &mut String, key: &str, value: &str) {
    buf.push_str(key);
    buf.push('\n');
    buf.push_str(value);
    buf.push('\n');
}

/// Build the exact byte string AWS signed, per the SNS signature spec: the
/// type-specific fields in lexical order, each as `key\nvalue\n`. `Subject`
/// is included only when present (Notifications without a subject omit it
/// entirely, they don't sign an empty value).
pub fn canonical_string(m: &SnsMessage) -> Result<String, SnsError> {
    let mut s = String::new();
    match m.type_.as_str() {
        TYPE_NOTIFICATION => {
            push_kv(&mut s, "Message", &m.message);
            push_kv(&mut s, "MessageId", &m.message_id);
            if let Some(subject) = &m.subject {
                push_kv(&mut s, "Subject", subject);
            }
            push_kv(&mut s, "Timestamp", &m.timestamp);
            push_kv(&mut s, "TopicArn", &m.topic_arn);
            push_kv(&mut s, "Type", &m.type_);
        }
        TYPE_SUBSCRIPTION_CONFIRMATION | TYPE_UNSUBSCRIBE_CONFIRMATION => {
            let subscribe_url = m
                .subscribe_url
                .as_deref()
                .ok_or(SnsError::MissingField("SubscribeURL"))?;
            let token = m.token.as_deref().ok_or(SnsError::MissingField("Token"))?;
            push_kv(&mut s, "Message", &m.message);
            push_kv(&mut s, "MessageId", &m.message_id);
            push_kv(&mut s, "SubscribeURL", subscribe_url);
            push_kv(&mut s, "Timestamp", &m.timestamp);
            push_kv(&mut s, "Token", token);
            push_kv(&mut s, "TopicArn", &m.topic_arn);
            push_kv(&mut s, "Type", &m.type_);
        }
        other => return Err(SnsError::UnknownType(other.to_string())),
    }
    Ok(s)
}

/// Constrain `SigningCertURL` to an AWS SNS host over HTTPS. This is the SSRF
/// guard: the URL comes from the (as-yet-unverified) message, so a forged
/// message must not be able to make us fetch a "certificate" from an
/// attacker-controlled host and then verify against it.
pub fn validate_cert_url(raw: &str) -> Result<reqwest::Url, SnsError> {
    let url = reqwest::Url::parse(raw).map_err(|_| SnsError::UntrustedCertUrl)?;
    if url.scheme() != "https" {
        return Err(SnsError::UntrustedCertUrl);
    }
    let host = url.host_str().ok_or(SnsError::UntrustedCertUrl)?;
    let trusted = host.starts_with("sns.")
        && (host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn"));
    if !trusted {
        return Err(SnsError::UntrustedCertUrl);
    }
    Ok(url)
}

/// Pull the RSA public key out of a PEM-encoded X.509 certificate. We only
/// read the SubjectPublicKeyInfo; this is not a TLS trust decision (the host
/// allowlist + signature check are the trust boundary).
pub fn rsa_public_key_from_cert_pem(pem: &str) -> Result<RsaPublicKey, SnsError> {
    let (_, parsed) =
        x509_parser::pem::parse_x509_pem(pem.as_bytes()).map_err(|_| SnsError::BadCertificate)?;
    let cert = parsed.parse_x509().map_err(|_| SnsError::BadCertificate)?;
    RsaPublicKey::from_public_key_der(cert.public_key().raw).map_err(|_| SnsError::BadCertificate)
}

/// Verify a base64 RSA-SHA256 signature over `canonical` using `key`.
pub fn verify_signature(
    canonical: &str,
    signature_b64: &str,
    key: &RsaPublicKey,
) -> Result<(), SnsError> {
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64.trim())
        .map_err(|_| SnsError::BadSignature)?;
    let signature =
        Signature::try_from(sig_bytes.as_slice()).map_err(|_| SnsError::BadSignature)?;
    VerifyingKey::<Sha256>::new(key.clone())
        .verify(canonical.as_bytes(), &signature)
        .map_err(|_| SnsError::VerificationFailed)
}

/// Process-wide cache of fetched signing certificates, keyed by URL. SNS
/// rotates these rarely and reuses one across many messages, so caching saves
/// a network round-trip per inbound email. The cert is public data; the cache
/// holds the PEM text.
static CERT_CACHE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

async fn fetch_cert_pem(http: &reqwest::Client, url: reqwest::Url) -> Result<String, SnsError> {
    let key = url.as_str().to_string();
    if let Some(pem) = CERT_CACHE.lock().unwrap().get(&key).cloned() {
        return Ok(pem);
    }
    let pem = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| SnsError::CertFetch(e.to_string()))?
        .text()
        .await
        .map_err(|e| SnsError::CertFetch(e.to_string()))?;
    // Validate before caching so a transient bad body isn't memoised.
    rsa_public_key_from_cert_pem(&pem)?;
    CERT_CACHE.lock().unwrap().insert(key, pem.clone());
    Ok(pem)
}

/// Full verification: require SignatureVersion 2, vet the cert URL, fetch (and
/// cache) the certificate, and verify the signature over the canonical string.
/// Returns `Ok(())` only for an authentic, untampered SNS message.
pub async fn verify_message(http: &reqwest::Client, m: &SnsMessage) -> Result<(), SnsError> {
    if m.signature_version != "2" {
        return Err(SnsError::UnsupportedSignatureVersion(
            m.signature_version.clone(),
        ));
    }
    let cert_url = validate_cert_url(&m.signing_cert_url)?;
    let canonical = canonical_string(m)?;
    let pem = fetch_cert_pem(http, cert_url).await?;
    let key = rsa_public_key_from_cert_pem(&pem)?;
    verify_signature(&canonical, &m.signature, &key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::RsaPrivateKey;

    const TEST_CERT_PEM: &str = include_str!("test_fixtures/sns_test_cert.pem");
    const TEST_KEY_PEM: &str = include_str!("test_fixtures/sns_test_key.pem");

    fn sign(canonical: &str) -> String {
        let key = RsaPrivateKey::from_pkcs8_pem(TEST_KEY_PEM).unwrap();
        let signing_key = SigningKey::<Sha256>::new(key);
        let sig = signing_key.sign(canonical.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
    }

    fn notification(subject: Option<&str>) -> SnsMessage {
        SnsMessage {
            type_: TYPE_NOTIFICATION.into(),
            message_id: "msg-1".into(),
            topic_arn: "arn:aws:sns:ap-southeast-2:111122223333:inbound".into(),
            message: "{\"hello\":\"world\"}".into(),
            timestamp: "2026-06-20T00:00:00.000Z".into(),
            signature_version: "2".into(),
            signature: String::new(),
            signing_cert_url:
                "https://sns.ap-southeast-2.amazonaws.com/SimpleNotificationService-abc.pem".into(),
            subject: subject.map(|s| s.to_string()),
            subscribe_url: None,
            token: None,
        }
    }

    #[test]
    fn canonical_notification_orders_fields_and_omits_absent_subject() {
        let m = notification(None);
        let c = canonical_string(&m).unwrap();
        assert_eq!(
            c,
            "Message\n{\"hello\":\"world\"}\nMessageId\nmsg-1\nTimestamp\n\
             2026-06-20T00:00:00.000Z\nTopicArn\n\
             arn:aws:sns:ap-southeast-2:111122223333:inbound\nType\nNotification\n"
        );
        // Subject is inserted (after MessageId) only when present.
        let with_subject = canonical_string(&notification(Some("hi"))).unwrap();
        assert!(with_subject.contains("MessageId\nmsg-1\nSubject\nhi\nTimestamp\n"));
    }

    #[test]
    fn canonical_subscription_confirmation_uses_subscribe_url_and_token() {
        let mut m = notification(None);
        m.type_ = TYPE_SUBSCRIPTION_CONFIRMATION.into();
        m.subscribe_url = Some("https://sns.example/confirm".into());
        m.token = Some("tok-123".into());
        let c = canonical_string(&m).unwrap();
        assert!(c.contains("SubscribeURL\nhttps://sns.example/confirm\n"));
        assert!(c.contains("Token\ntok-123\n"));
        // Notification-only assembly would have omitted these.
        assert!(c.starts_with("Message\n"));
    }

    #[test]
    fn subscription_confirmation_missing_token_errors() {
        let mut m = notification(None);
        m.type_ = TYPE_SUBSCRIPTION_CONFIRMATION.into();
        m.subscribe_url = Some("https://sns.example/confirm".into());
        m.token = None;
        assert!(matches!(
            canonical_string(&m),
            Err(SnsError::MissingField("Token"))
        ));
    }

    #[test]
    fn cert_url_allowlist_accepts_aws_rejects_others() {
        assert!(validate_cert_url(
            "https://sns.ap-southeast-2.amazonaws.com/SimpleNotificationService-x.pem"
        )
        .is_ok());
        assert!(validate_cert_url("https://sns.cn-north-1.amazonaws.com.cn/x.pem").is_ok());
        // http, wrong host, and lookalike suffixes are all rejected.
        assert!(validate_cert_url("http://sns.ap-southeast-2.amazonaws.com/x.pem").is_err());
        assert!(validate_cert_url("https://evil.amazonaws.com/x.pem").is_err());
        assert!(
            validate_cert_url("https://sns.ap-southeast-2.amazonaws.com.evil.com/x.pem").is_err()
        );
        assert!(validate_cert_url("https://attacker.com/sns.amazonaws.com").is_err());
    }

    #[test]
    fn round_trip_signature_verifies_through_cert() {
        let mut m = notification(None);
        let canonical = canonical_string(&m).unwrap();
        m.signature = sign(&canonical);

        let key = rsa_public_key_from_cert_pem(TEST_CERT_PEM).unwrap();
        assert!(verify_signature(&canonical, &m.signature, &key).is_ok());
    }

    #[test]
    fn tampered_message_fails_verification() {
        let m = notification(None);
        let canonical = canonical_string(&m).unwrap();
        let signature = sign(&canonical);

        // Same signature, different payload: must not verify.
        let mut tampered = m.clone();
        tampered.message = "{\"hello\":\"evil\"}".into();
        let tampered_canonical = canonical_string(&tampered).unwrap();

        let key = rsa_public_key_from_cert_pem(TEST_CERT_PEM).unwrap();
        assert!(matches!(
            verify_signature(&tampered_canonical, &signature, &key),
            Err(SnsError::VerificationFailed)
        ));
    }

    #[test]
    fn garbage_signature_is_rejected() {
        let key = rsa_public_key_from_cert_pem(TEST_CERT_PEM).unwrap();
        assert!(matches!(
            verify_signature("anything", "not-base64-!!!", &key),
            Err(SnsError::BadSignature)
        ));
    }

    #[test]
    fn parse_rejects_non_json() {
        assert!(matches!(
            SnsMessage::parse(b"not json"),
            Err(SnsError::Malformed(_))
        ));
    }
}
