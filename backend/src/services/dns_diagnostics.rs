//! Live email-authentication diagnostics for a sending domain.
//!
//! Backs the admin "DNS health" panel: given a workspace's verified sending
//! domain, resolve and classify its SPF, DKIM, DMARC, and MX records so the
//! admin can self-diagnose deliverability without leaving the app. Read-only;
//! it never writes status (the authoritative DKIM flip is `dkim_verification`).
//!
//! Like the DKIM check, these are public-DNS lookups of an admin-supplied
//! domain, not outbound connections to a tenant host, so they use hickory
//! directly and don't go through the HTTP SSRF egress guard.

use serde::Serialize;

use crate::services::dkim_verification::{published_record_has_key, txt_lookup};

/// Status of one record check. Maps to a colour in the UI.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Present and correct.
    Pass,
    /// Missing but recommended (e.g. no DMARC).
    Warn,
    /// Missing or wrong where it's required (e.g. DKIM not published).
    Fail,
    /// Informational; no action implied (e.g. SPF/MX state, or a lookup error).
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordCheck {
    pub status: CheckStatus,
    /// One-line human-readable result.
    pub summary: String,
    /// The raw record found, when there is one to show.
    pub value: Option<String>,
}

impl RecordCheck {
    fn new(status: CheckStatus, summary: impl Into<String>, value: Option<String>) -> Self {
        Self {
            status,
            summary: summary.into(),
            value,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailAuthReport {
    pub domain: String,
    pub spf: RecordCheck,
    pub dkim: RecordCheck,
    pub dmarc: RecordCheck,
    pub mx: RecordCheck,
}

/// Run all four checks for `domain`. `dkim_record_name` is the full
/// `<selector>._domainkey.<domain>` name and `dkim_expected_b64` is our public
/// key, both from `workspace_email_settings::dns_record_for`.
pub async fn check_email_auth(
    domain: &str,
    dkim_record_name: &str,
    dkim_expected_b64: &str,
) -> EmailAuthReport {
    let dmarc_name = format!("_dmarc.{domain}");
    let (spf, dkim, dmarc, mx) = tokio::join!(
        txt_lookup(domain),
        txt_lookup(dkim_record_name),
        txt_lookup(&dmarc_name),
        mx_lookup(domain),
    );

    EmailAuthReport {
        domain: domain.to_string(),
        spf: classify_spf(spf),
        dkim: classify_dkim(dkim, dkim_expected_b64),
        dmarc: classify_dmarc(dmarc),
        mx: classify_mx(mx),
    }
}

type TxtResult = Result<Vec<String>, crate::services::dkim_verification::VerifyError>;

fn classify_spf(result: TxtResult) -> RecordCheck {
    match result {
        Err(e) => RecordCheck::new(CheckStatus::Info, format!("Couldn't check SPF: {e}"), None),
        Ok(records) => match records
            .iter()
            .find(|r| r.trim_start().to_ascii_lowercase().starts_with("v=spf1"))
        {
            Some(spf) => {
                RecordCheck::new(CheckStatus::Pass, "SPF record found.", Some(spf.clone()))
            }
            // SPF isn't required: DMARC passes on DKIM alignment alone. So its
            // absence is informational, not a warning.
            None => RecordCheck::new(
                CheckStatus::Info,
                "No SPF record. Not required here: DMARC passes on DKIM alignment.",
                None,
            ),
        },
    }
}

fn classify_dkim(result: TxtResult, expected_b64: &str) -> RecordCheck {
    match result {
        Err(e) => RecordCheck::new(CheckStatus::Info, format!("Couldn't check DKIM: {e}"), None),
        Ok(records) if published_record_has_key(&records, expected_b64) => RecordCheck::new(
            CheckStatus::Pass,
            "DKIM record published and matches.",
            None,
        ),
        Ok(records) if records.is_empty() => RecordCheck::new(
            CheckStatus::Fail,
            "DKIM record not found at the selector. Publish the record shown above.",
            None,
        ),
        Ok(records) => RecordCheck::new(
            CheckStatus::Fail,
            "A record exists at the selector but doesn't match our key.",
            records.into_iter().next(),
        ),
    }
}

fn classify_dmarc(result: TxtResult) -> RecordCheck {
    match result {
        Err(e) => RecordCheck::new(
            CheckStatus::Info,
            format!("Couldn't check DMARC: {e}"),
            None,
        ),
        Ok(records) => match records
            .iter()
            .find(|r| r.trim_start().to_ascii_lowercase().starts_with("v=dmarc1"))
        {
            Some(dmarc) => {
                let policy = dmarc_policy(dmarc).unwrap_or_else(|| "none".to_string());
                RecordCheck::new(
                    CheckStatus::Pass,
                    format!("DMARC record found (policy: {policy})."),
                    Some(dmarc.clone()),
                )
            }
            None => RecordCheck::new(
                CheckStatus::Warn,
                "No DMARC record. Recommended so receivers know how to treat unauthenticated mail.",
                None,
            ),
        },
    }
}

fn classify_mx(result: Result<Vec<String>, String>) -> RecordCheck {
    match result {
        Err(e) => RecordCheck::new(CheckStatus::Info, format!("Couldn't check MX: {e}"), None),
        Ok(hosts) if hosts.is_empty() => RecordCheck::new(
            CheckStatus::Info,
            "No MX records (the domain can't receive mail). Fine for a send-only domain.",
            None,
        ),
        Ok(hosts) => {
            let n = hosts.len();
            RecordCheck::new(
                CheckStatus::Info,
                format!("{n} MX host(s) found."),
                Some(hosts.join(", ")),
            )
        }
    }
}

/// Extract the `p=` policy tag from a DMARC record (`none` / `quarantine` /
/// `reject`). Tags are `;`-separated `name=value`.
fn dmarc_policy(record: &str) -> Option<String> {
    record
        .split(';')
        .filter_map(|part| part.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("p"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())
}

/// Resolve MX hostnames at `name`, sorted by preference. NXDOMAIN / no-records
/// yields an empty list, mirroring `txt_lookup`.
async fn mx_lookup(name: &str) -> Result<Vec<String>, String> {
    use hickory_resolver::net::{DnsError, NetError};
    use hickory_resolver::proto::rr::RData;

    // One shared resolver for the whole email-auth DNS surface (see
    // `dkim_verification::email_auth_resolver` for why it's the host resolver).
    let resolver = crate::services::dkim_verification::email_auth_resolver()?;

    match resolver.mx_lookup(name).await {
        Ok(lookup) => {
            let mut hosts: Vec<(u16, String)> = lookup
                .answers()
                .iter()
                .filter_map(|rec| match &rec.data {
                    RData::MX(mx) => Some((mx.preference, mx.exchange.to_string())),
                    _ => None,
                })
                .collect();
            hosts.sort_by_key(|(pref, _)| *pref);
            Ok(hosts.into_iter().map(|(_, host)| host).collect())
        }
        Err(NetError::Dns(DnsError::NoRecordsFound(_))) => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmarc_policy_extracts_p_tag() {
        assert_eq!(
            dmarc_policy("v=DMARC1; p=reject; rua=mailto:x@y.com").as_deref(),
            Some("reject")
        );
        assert_eq!(
            dmarc_policy("v=DMARC1;p=quarantine").as_deref(),
            Some("quarantine")
        );
        assert_eq!(dmarc_policy("v=DMARC1; rua=mailto:x@y.com"), None);
    }

    #[test]
    fn classify_spf_finds_record_case_insensitively() {
        let pass = classify_spf(Ok(vec!["V=SPF1 include:amazonses.com -all".into()]));
        assert!(matches!(pass.status, CheckStatus::Pass));
        let none = classify_spf(Ok(vec!["some other txt".into()]));
        assert!(matches!(none.status, CheckStatus::Info));
    }

    #[test]
    fn classify_dmarc_warns_when_absent() {
        let warn = classify_dmarc(Ok(vec![]));
        assert!(matches!(warn.status, CheckStatus::Warn));
        let pass = classify_dmarc(Ok(vec!["v=DMARC1; p=none".into()]));
        assert!(matches!(pass.status, CheckStatus::Pass));
        assert!(pass.summary.contains("none"));
    }

    #[test]
    fn classify_dkim_fails_when_absent_or_mismatched() {
        let key = "MIIBIjANBgkqhkiG9w0BAQEF";
        let pass = classify_dkim(Ok(vec![format!("v=DKIM1; k=rsa; p={key}")]), key);
        assert!(matches!(pass.status, CheckStatus::Pass));

        let absent = classify_dkim(Ok(vec![]), key);
        assert!(matches!(absent.status, CheckStatus::Fail));

        let mismatch = classify_dkim(Ok(vec!["v=DKIM1; k=rsa; p=OTHER".into()]), key);
        assert!(matches!(mismatch.status, CheckStatus::Fail));
        assert!(mismatch.value.is_some());
    }
}
