//! SSRF-gated LDAP connector (P0.5 de-risking spike).
//!
//! Spike finding: ldap3 0.11's async `LdapConnAsync` does its own
//! `TcpStream::connect` from the URL and verifies the TLS cert against the URL
//! host, with NO API to hand it a pre-established stream. So the clean
//! "connect to an egress-validated IP, then verify the cert against the
//! hostname" handoff (the IMAP pattern in `email_imap.rs`) is not available
//! here.
//!
//! Instead we gate with [`egress::resolve_and_validate`] as a PRE-FLIGHT: it
//! rejects a host that resolves to a non-routable IP unless the operator
//! allowlisted it (`NOSDESK_OUTBOUND_ALLOWED_HOSTS`), which is exactly how a
//! self-hosted admin permits their on-prem domain controller. ldap3 then
//! connects by hostname, so certificate hostname verification stays correct.
//!
//! The residual is a DNS-rebind TOCTOU (the host resolves routable at the
//! pre-flight, private by the time ldap3 re-resolves and connects). This is
//! acceptable for v1: self-hosted is the only LDAP consumer (cloud uses SCIM),
//! the host is allowlisted by the admin who controls that network, and the
//! deferred cloud connector agent (P6) dials OUT from the customer network with
//! no inbound SSRF surface. If cloud ever dials LDAP directly, revisit this.

use std::time::Duration;

use ldap3::{Ldap, LdapConnAsync, LdapConnSettings};
use tracing::warn;

use crate::models::WorkspaceLdapSettings;
use crate::utils::egress::{self, EgressError};

/// Per-operation timeout for binds + searches. `set_conn_timeout` only bounds
/// the TCP/TLS connect; ldap3 defaults the live handle to NO op timeout, so a
/// DC that completes the handshake then stalls a BIND/SEARCH would hang the
/// await forever. Callers apply this via `ldap.with_timeout(OP_TIMEOUT)` before
/// each op (ldap3 consumes the timeout per operation).
pub const OP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum LdapConnectError {
    #[error("LDAP host rejected by egress policy: {0}")]
    Egress(#[from] EgressError),
    #[error("invalid LDAP TLS configuration: {0}")]
    Tls(String),
    #[error("LDAP connection failed: {0}")]
    Ldap(#[from] ldap3::LdapError),
}

/// Open an SSRF-gated connection to the workspace's LDAP server and return the
/// live [`Ldap`] handle (its driver is spawned on the current Tokio runtime).
/// The caller binds + searches. Connect/op timeouts come from the settings;
/// LDAPS (`ldaps`) wraps TLS from the first byte, `starttls` upgrades a plain
/// connection before the bind.
pub async fn connect(settings: &WorkspaceLdapSettings) -> Result<Ldap, LdapConnectError> {
    let host = settings.host.trim();
    let port = settings.port as u16;

    // SSRF pre-flight: same chokepoint as the IMAP raw-TCP path. An allowlisted
    // host (the on-prem DC) passes even though it's on RFC1918.
    egress::resolve_and_validate(host, port).await?;

    // Only encrypted transports: LDAPS wraps TLS from the first byte, StartTLS
    // upgrades a plain ldap:// connection BEFORE the bind. A cleartext "plain"
    // bind is never offered — it would ship the service + user passwords over an
    // unencrypted socket (RFC 4513 §5.1.3), so it's rejected here as well as in
    // the admin validator.
    let scheme = match settings.tls_mode.as_str() {
        "ldaps" => "ldaps",
        "starttls" => "ldap",
        other => {
            return Err(LdapConnectError::Tls(format!(
                "unsupported tls_mode '{other}' (must be 'ldaps' or 'starttls')"
            )))
        }
    };
    let url = format!("{scheme}://{host}:{port}");

    let mut conn_settings = LdapConnSettings::new()
        .set_conn_timeout(Duration::from_secs(
            settings.connect_timeout_secs.max(1) as u64
        ))
        .set_starttls(settings.tls_mode == "starttls");

    // The cert-verify bypass is for internal self-signed CAs and is FORCED OFF
    // in production, mirroring the IMAP channel gate; prefer a supplied CA cert.
    let skip_verify = !settings.verify_certs && !crate::config_utils::is_production();
    if !settings.verify_certs && !skip_verify {
        warn!(
            host,
            "LDAP verify_certs=false ignored: the deployment is production. \
             Supply ca_cert_pem for an internal CA instead."
        );
    }
    if skip_verify {
        conn_settings = conn_settings.set_no_tls_verify(true);
    }

    if let Some(ca_pem) = settings
        .ca_cert_pem
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let cert = native_tls::Certificate::from_pem(ca_pem.as_bytes())
            .map_err(|e| LdapConnectError::Tls(format!("ca_cert_pem parse: {e}")))?;
        let connector = native_tls::TlsConnector::builder()
            .add_root_certificate(cert)
            .build()
            .map_err(|e| LdapConnectError::Tls(format!("tls connector build: {e}")))?;
        conn_settings = conn_settings.set_connector(connector);
    }

    let (conn, ldap) = LdapConnAsync::with_settings(conn_settings, &url).await?;
    ldap3::drive!(conn);
    Ok(ldap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings_for(host: &str) -> WorkspaceLdapSettings {
        WorkspaceLdapSettings {
            workspace_id: 1,
            enabled: true,
            host: host.to_string(),
            port: 636,
            tls_mode: "ldaps".into(),
            verify_certs: true,
            ca_cert_pem: None,
            follow_referrals: false,
            connect_timeout_secs: 3,
            auth_mode: "simple_bind".into(),
            bind_dn: String::new(),
            encrypted_bind_password: None,
            encrypted_kek_id: None,
            user_base_dn: String::new(),
            username_attribute: "uid".into(),
            user_filter: "(uid={username})".into(),
            page_size: 500,
            attribute_map: json!({}),
            group_config: json!({}),
            provisioning: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // The SSRF pre-flight rejects a loopback host before any LDAP dial. This is
    // the core de-risk: the egress gate runs first and short-circuits.
    #[tokio::test]
    async fn rejects_a_loopback_host_at_the_egress_gate() {
        let err = connect(&settings_for("127.0.0.1")).await.unwrap_err();
        assert!(
            matches!(err, LdapConnectError::Egress(_)),
            "loopback must be rejected by the egress pre-flight, got {err:?}"
        );
    }

    // An unresolvable host fails at the egress gate (DNS lookup), not later in
    // ldap3, confirming the pre-flight runs before the dial.
    #[tokio::test]
    async fn rejects_an_unresolvable_host_at_the_egress_gate() {
        let err = connect(&settings_for("nosdesk-ldap-does-not-exist.invalid"))
            .await
            .unwrap_err();
        assert!(matches!(err, LdapConnectError::Egress(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn rejects_an_unknown_tls_mode() {
        let mut s = settings_for("ldap.example.com");
        s.tls_mode = "bogus".into();
        // A bad tls_mode would be a Tls error, but the egress gate runs first;
        // use an allowlistable literal that still resolves to a routable IP so
        // we reach the tls_mode check. 8.8.8.8 is globally routable.
        s.host = "8.8.8.8".into();
        let err = connect(&s).await.unwrap_err();
        assert!(matches!(err, LdapConnectError::Tls(_)), "got {err:?}");
    }
}
