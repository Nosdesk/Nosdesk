//! DKIM domain verification.
//!
//! A workspace publishes our public key at `<selector>._domainkey.<domain>`.
//! This module resolves that TXT record and compares the published `p=` key to
//! ours, flipping `verification_status` between `pending` and `verified`.
//!
//! A DKIM TXT lookup queries public DNS for a record of an admin-supplied
//! domain; it is not an outbound connection to a tenant host, so it does not
//! go through the HTTP SSRF egress guard. We use hickory directly, mirroring
//! the guest MX pre-flight in `handlers::guest`.

use std::time::Duration;

use crate::db::Pool;
use crate::models::workspace_email_verification_status as status;
use crate::repository::workspace_email_settings as ws_settings;
use crate::sync::session::{run_in_workspace, BackgroundRunError};

#[derive(Debug)]
pub enum VerifyError {
    /// The workspace isn't in verified-domain mode / has no DKIM key.
    NotProvisioned,
    /// DNS lookup failed (resolver build or a transient error).
    Dns(String),
    /// Reading or writing the settings row failed.
    Background(BackgroundRunError),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotProvisioned => write!(f, "workspace has no DKIM domain to verify"),
            Self::Dns(e) => write!(f, "DNS lookup: {e}"),
            Self::Background(e) => write!(f, "verification db: {e}"),
        }
    }
}
impl std::error::Error for VerifyError {}

/// Strip all whitespace. DNS UIs wrap long `p=` values and split TXT into
/// <=255-char segments, so the key is compared whitespace-insensitively.
fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Extract the `p=` (public key) value from a DKIM TXT record, whitespace
/// stripped, or `None` if absent. Tags are `;`-separated `name=value`; the
/// `p=` value's base64 padding (`=`) is preserved because we split on the
/// first `=` only.
fn extract_p_tag(txt: &str) -> Option<String> {
    txt.split(';')
        .filter_map(|part| part.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("p"))
        .map(|(_, v)| strip_ws(v))
}

/// True when any published TXT record carries our exact public key in its
/// `p=` tag. Compares the key, not the literal record, since providers
/// reformat TXT (segment splits, added whitespace, reordered tags).
pub fn published_record_has_key(published: &[String], expected_public_b64: &str) -> bool {
    let expected = strip_ws(expected_public_b64);
    published
        .iter()
        .filter_map(|txt| extract_p_tag(txt))
        .any(|p| p == expected)
}

/// Resolve the TXT records at `name`. NXDOMAIN / no-records yields an empty
/// list (the record isn't published yet), not an error.
async fn txt_lookup(name: &str) -> Result<Vec<String>, VerifyError> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    use hickory_resolver::net::{runtime::TokioRuntimeProvider, DnsError, NetError};
    use hickory_resolver::proto::rr::RData;
    use hickory_resolver::TokioResolver;

    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(5);
    opts.attempts = 2;
    let resolver = TokioResolver::builder_with_config(
        ResolverConfig::default(),
        TokioRuntimeProvider::default(),
    )
    .with_options(opts)
    .build()
    .map_err(|e| VerifyError::Dns(format!("resolver build: {e}")))?;

    match resolver.txt_lookup(name).await {
        // `TXT`'s Display concatenates its <=255-char segments into the full
        // record value.
        Ok(lookup) => Ok(lookup
            .answers()
            .iter()
            .filter_map(|rec| match &rec.data {
                RData::TXT(txt) => Some(txt.to_string()),
                _ => None,
            })
            .collect()),
        Err(NetError::Dns(DnsError::NoRecordsFound(_))) => Ok(Vec::new()),
        Err(e) => Err(VerifyError::Dns(e.to_string())),
    }
}

/// Verify the workspace's DKIM domain: resolve the published record, compare
/// it to our key, and persist `verified` or `pending`. Returns the new status.
/// Reads the expected record on a pinned connection and releases it before the
/// (slow) DNS lookup, then writes the result on a fresh one.
pub async fn verify_dkim_domain(
    pool: &Pool,
    workspace_id: i32,
) -> Result<&'static str, VerifyError> {
    let record = run_in_workspace(pool, "dkim-verify-read", workspace_id, |conn| {
        let row = ws_settings::get_for_workspace(conn, workspace_id)?
            .ok_or(diesel::result::Error::NotFound)?;
        ws_settings::dns_record_for(&row)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .map_err(VerifyError::Background)?
    .ok_or(VerifyError::NotProvisioned)?;

    let published = txt_lookup(&record.name).await?;
    let verified = published_record_has_key(&published, &record.public_b64);
    let new_status = if verified {
        status::VERIFIED
    } else {
        status::PENDING
    };
    let verified_at = verified.then(|| chrono::Utc::now().naive_utc());

    run_in_workspace(pool, "dkim-verify-write", workspace_id, |conn| {
        ws_settings::set_verification_status(conn, workspace_id, new_status, verified_at)
    })
    .map_err(VerifyError::Background)?;

    Ok(new_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "MIIBIjANBgkqhkiG9w0BAQEF";

    #[test]
    fn matches_exact_record() {
        assert!(published_record_has_key(
            &[format!("v=DKIM1; k=rsa; p={KEY}")],
            KEY
        ));
    }

    #[test]
    fn matches_with_wrapped_whitespace_in_key() {
        // Some DNS UIs insert spaces / tabs into a long p= value.
        assert!(published_record_has_key(
            &["v=DKIM1; k=rsa; p=MIIBIjAN Bgkqhki\tG9w0BAQEF".to_string()],
            KEY
        ));
    }

    #[test]
    fn matches_with_reordered_and_extra_tags() {
        assert!(published_record_has_key(
            &[format!("k=rsa; t=s; p={KEY}; s=email")],
            KEY
        ));
    }

    #[test]
    fn matches_when_expected_has_whitespace() {
        assert!(published_record_has_key(
            &[format!("v=DKIM1; k=rsa; p={KEY}")],
            "MIIBIjAN Bgkqhki G9w0BAQEF"
        ));
    }

    #[test]
    fn no_match_for_different_key() {
        assert!(!published_record_has_key(
            &["v=DKIM1; k=rsa; p=DIFFERENTKEY".to_string()],
            KEY
        ));
    }

    #[test]
    fn no_match_without_p_tag() {
        assert!(!published_record_has_key(
            &["v=DKIM1; k=rsa".to_string()],
            KEY
        ));
    }

    #[test]
    fn no_match_when_unpublished() {
        assert!(!published_record_has_key(&[], KEY));
    }

    #[test]
    fn matches_among_multiple_records() {
        assert!(published_record_has_key(
            &[
                "v=DKIM1; k=rsa; p=OLDKEY".to_string(),
                format!("v=DKIM1; k=rsa; p={KEY}"),
            ],
            KEY
        ));
    }

    #[test]
    fn preserves_base64_padding() {
        let key = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQE=";
        assert!(published_record_has_key(
            &[format!("v=DKIM1; k=rsa; p={key}")],
            key
        ));
    }
}
