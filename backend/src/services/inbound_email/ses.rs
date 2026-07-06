//! Parsing the SES "Received" event that rides inside an SNS notification,
//! plus the pure routing/gating decisions taken from it.
//!
//! When SES receives mail for our inbound domain it runs the receipt rule's
//! S3 action (writing the raw MIME to S3) and publishes a notification to SNS.
//! The notification's `Message` field is a JSON string holding this event:
//! the envelope recipients (how we route), the spam/virus verdicts (how we
//! gate), and the S3 object location (where the raw mail is).

use serde::Deserialize;

/// The SES event carried in `SnsMessage.message`.
#[derive(Debug, Clone, Deserialize)]
pub struct SesNotification {
    pub mail: SesMail,
    pub receipt: SesReceipt,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SesMail {
    /// Envelope recipients (the addresses SES received for). Authoritative for
    /// routing; the `To:` header is unreliable after forwarding.
    #[serde(default)]
    pub destination: Vec<String>,
    /// Envelope sender.
    pub source: Option<String>,
    #[serde(rename = "commonHeaders", default)]
    pub common_headers: Option<SesCommonHeaders>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SesCommonHeaders {
    #[serde(default)]
    pub from: Vec<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SesReceipt {
    /// Recipients this receipt rule matched — the same envelope addresses,
    /// used for routing.
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(rename = "spamVerdict")]
    pub spam_verdict: Option<SesVerdict>,
    #[serde(rename = "virusVerdict")]
    pub virus_verdict: Option<SesVerdict>,
    pub action: Option<SesAction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SesVerdict {
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SesAction {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(rename = "bucketName")]
    pub bucket_name: Option<String>,
    #[serde(rename = "objectKey")]
    pub object_key: Option<String>,
}

impl SesNotification {
    /// Parse the SES event from the SNS `Message` string.
    pub fn parse(message: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(message)
    }

    /// The first envelope recipient (receipt recipients take precedence over
    /// `mail.destination`; they're the same in practice).
    pub fn first_recipient(&self) -> Option<&str> {
        self.receipt
            .recipients
            .first()
            .or_else(|| self.mail.destination.first())
            .map(|s| s.as_str())
    }

    /// The S3 object key the raw MIME was written to, if the triggering action
    /// was an S3 action.
    pub fn s3_object_key(&self) -> Option<&str> {
        self.receipt.action.as_ref()?.object_key.as_deref()
    }

    /// Sender address from the envelope, falling back to the `From:` header.
    pub fn sender(&self) -> Option<String> {
        if let Some(src) = &self.mail.source {
            if !src.is_empty() {
                return Some(src.clone());
            }
        }
        self.mail
            .common_headers
            .as_ref()
            .and_then(|h| h.from.first().cloned())
    }

    pub fn subject(&self) -> Option<String> {
        self.mail
            .common_headers
            .as_ref()
            .and_then(|h| h.subject.clone())
    }
}

fn verdict_failed(v: &Option<SesVerdict>) -> bool {
    v.as_ref()
        .and_then(|v| v.status.as_deref())
        .map(|s| s.eq_ignore_ascii_case("FAIL"))
        .unwrap_or(false)
}

/// Virus scan failed. Always a hard drop (we never ingest malware), regardless
/// of whether the recipient token is known. A missing verdict is a pass: SES
/// only omits verdicts when scanning is disabled, which is an operator choice.
pub fn virus_failed(receipt: &SesReceipt) -> bool {
    verdict_failed(&receipt.virus_verdict)
}

/// Spam scan failed. NOT a hard drop on its own: for a *known* workspace the
/// caller still opens a flagged ticket, because a false positive — common with
/// forwarded mail, which breaks SPF alignment — must not silently lose a real
/// customer request. Only mail to an *unknown* token is dropped on a spam FAIL.
pub fn spam_failed(receipt: &SesReceipt) -> bool {
    verdict_failed(&receipt.spam_verdict)
}

/// Extract the forwarding token from an envelope recipient of the form
/// `<token>@<inbound_domain>`. Case-insensitive on the domain; returns the
/// localpart as the token. `None` if the recipient isn't on our inbound
/// domain (which shouldn't happen given the SES receipt rule, but we don't
/// assume).
pub fn token_from_recipient(recipient: &str, inbound_domain: &str) -> Option<String> {
    let (local, domain) = recipient.rsplit_once('@')?;
    if local.is_empty() || !domain.eq_ignore_ascii_case(inbound_domain) {
        return None;
    }
    Some(local.to_string())
}

/// First recipient that resolves to a token on our inbound domain.
pub fn first_token(recipients: &[String], inbound_domain: &str) -> Option<String> {
    recipients
        .iter()
        .find_map(|r| token_from_recipient(r, inbound_domain))
}

/// Extract the workspace-slug candidate from a managed-address recipient of
/// the form `support[+tag]@<slug>.<tenant_domain>`. Returns the lowercased
/// slug label. `None` when the recipient isn't a managed address:
/// - localpart must be `support` or `support+<tag>` (case-insensitive; the
///   plus-tag routes nothing here — the threading cascade reads it from the
///   preserved recipient list),
/// - domain must be exactly one label under `tenant_domain`, matched anchored
///   (suffix-stripped, dot required) so `x@evil-nosdesk.app` and
///   `x@a.b.nosdesk.app` never resolve,
/// - the bare tenant domain itself never matches (no slug label).
pub fn managed_slug_from_recipient(recipient: &str, tenant_domain: &str) -> Option<String> {
    if tenant_domain.is_empty() {
        return None;
    }
    let (local, domain) = recipient.rsplit_once('@')?;
    let local = local.to_ascii_lowercase();
    let is_managed_local = local == crate::utils::tenant_origin::MANAGED_LOCALPART
        || local
            .strip_prefix(crate::utils::tenant_origin::MANAGED_LOCALPART)
            .is_some_and(|rest| rest.starts_with('+'));
    if !is_managed_local {
        return None;
    }
    let domain = domain.to_ascii_lowercase();
    let label = domain
        .strip_suffix(&tenant_domain.to_ascii_lowercase())?
        .strip_suffix('.')?;
    if label.is_empty() || label.contains('.') {
        return None;
    }
    Some(label.to_string())
}

/// First recipient that resolves to a managed-address slug candidate.
pub fn first_managed_slug(recipients: &[String], tenant_domain: &str) -> Option<String> {
    recipients
        .iter()
        .find_map(|r| managed_slug_from_recipient(r, tenant_domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "mail": {
            "source": "customer@theirco.com",
            "destination": ["abc123token@inbound.nosdesk.com"],
            "commonHeaders": {
                "from": ["A Customer <customer@theirco.com>"],
                "subject": "Help please"
            }
        },
        "receipt": {
            "recipients": ["abc123token@inbound.nosdesk.com"],
            "spamVerdict": {"status": "PASS"},
            "virusVerdict": {"status": "PASS"},
            "action": {"type": "S3", "bucketName": "nosdesk-inbound", "objectKey": "raw/abc123"}
        }
    }"#;

    #[test]
    fn parses_ses_event_fields() {
        let n = SesNotification::parse(SAMPLE).unwrap();
        assert_eq!(n.first_recipient(), Some("abc123token@inbound.nosdesk.com"));
        assert_eq!(n.s3_object_key(), Some("raw/abc123"));
        assert_eq!(n.sender().as_deref(), Some("customer@theirco.com"));
        assert_eq!(n.subject().as_deref(), Some("Help please"));
        assert!(!virus_failed(&n.receipt) && !spam_failed(&n.receipt));
    }

    #[test]
    fn verdict_helpers_detect_fail_case_insensitively() {
        let mut n = SesNotification::parse(SAMPLE).unwrap();
        assert!(!virus_failed(&n.receipt) && !spam_failed(&n.receipt));
        n.receipt.spam_verdict = Some(SesVerdict {
            status: Some("FAIL".into()),
        });
        assert!(spam_failed(&n.receipt) && !virus_failed(&n.receipt));
        n.receipt.virus_verdict = Some(SesVerdict {
            status: Some("fail".into()),
        });
        assert!(virus_failed(&n.receipt));
    }

    #[test]
    fn missing_verdicts_are_not_failures() {
        let mut n = SesNotification::parse(SAMPLE).unwrap();
        n.receipt.spam_verdict = None;
        n.receipt.virus_verdict = None;
        assert!(!virus_failed(&n.receipt) && !spam_failed(&n.receipt));
    }

    #[test]
    fn token_extraction_matches_domain_case_insensitively() {
        assert_eq!(
            token_from_recipient("abc123@inbound.nosdesk.com", "inbound.nosdesk.com").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            token_from_recipient("abc123@INBOUND.NOSDESK.COM", "inbound.nosdesk.com").as_deref(),
            Some("abc123")
        );
        // Wrong domain, empty localpart, and no @ all yield None.
        assert!(token_from_recipient("abc123@evil.com", "inbound.nosdesk.com").is_none());
        assert!(token_from_recipient("@inbound.nosdesk.com", "inbound.nosdesk.com").is_none());
        assert!(token_from_recipient("not-an-address", "inbound.nosdesk.com").is_none());
    }

    #[test]
    fn managed_slug_extraction_accepts_support_and_plus_tags() {
        assert_eq!(
            managed_slug_from_recipient("support@acme.nosdesk.app", "nosdesk.app").as_deref(),
            Some("acme")
        );
        assert_eq!(
            managed_slug_from_recipient("Support+ticket-42@ACME.nosdesk.app", "nosdesk.app")
                .as_deref(),
            Some("acme"),
            "case-insensitive; plus-tag ignored for routing"
        );
    }

    #[test]
    fn managed_slug_extraction_rejects_non_managed_addresses() {
        // Wrong localpart.
        assert!(managed_slug_from_recipient("sales@acme.nosdesk.app", "nosdesk.app").is_none());
        // `supportx` must not pass the prefix check.
        assert!(managed_slug_from_recipient("supportx@acme.nosdesk.app", "nosdesk.app").is_none());
        // Bare tenant domain: no slug label.
        assert!(managed_slug_from_recipient("support@nosdesk.app", "nosdesk.app").is_none());
        // Multi-label: not a direct child of the tenant domain.
        assert!(managed_slug_from_recipient("support@a.b.nosdesk.app", "nosdesk.app").is_none());
        // Anchoring: a lookalike suffix domain must not match.
        assert!(
            managed_slug_from_recipient("support@evil-nosdesk.app", "nosdesk.app").is_none(),
            "suffix match must require the dot boundary"
        );
        assert!(managed_slug_from_recipient("support@acme.nosdesk.app", "").is_none());
    }

    #[test]
    fn first_managed_slug_scans_recipients() {
        let recipients = vec![
            "noise@elsewhere.com".to_string(),
            "tok9@inbound.nosdesk.app".to_string(),
            "support@acme.nosdesk.app".to_string(),
        ];
        assert_eq!(
            first_managed_slug(&recipients, "nosdesk.app").as_deref(),
            Some("acme")
        );
    }

    #[test]
    fn first_token_scans_recipients() {
        let recipients = vec![
            "noise@elsewhere.com".to_string(),
            "tok9@inbound.nosdesk.com".to_string(),
        ];
        assert_eq!(
            first_token(&recipients, "inbound.nosdesk.com").as_deref(),
            Some("tok9")
        );
    }
}
