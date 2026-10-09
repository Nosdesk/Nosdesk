//! The dedicated `tokio_postgres` connections that hold a `LISTEN`.
//!
//! Live updates, the email queue, the notification outbox and the search
//! replicator each keep one beside the Diesel pool, because libpq's client
//! doesn't expose async notifications. They all connect here, with TLS chosen
//! the way libpq chooses it for the pool, from the URL's `sslmode` and
//! `sslrootcert`:
//!
//! | `sslmode` | TLS | checks |
//! |---|---|---|
//! | `disable` | never | |
//! | `allow`, `prefer`, unset | when the server offers it | none |
//! | `require` | always | none, or the chain when `sslrootcert` is set |
//! | `verify-ca` | always | the chain |
//! | `verify-full` | always | the chain and the host name |
//!
//! The chain is checked against `sslrootcert` (a PEM file, which may hold
//! several certificates), or the system roots when it's unset or `system`.
//! Two differences from libpq: `allow` tries TLS first, as `prefer` does, and
//! `prefer` falls back to plaintext only when the server declines TLS, not
//! when a handshake fails. Client certificates (`sslcert`, `sslkey`) aren't
//! supported here and are refused with a clear error.

use std::path::PathBuf;

use anyhow::{bail, Context};
use futures::stream::BoxStream;
use futures::StreamExt;
use native_tls::{Certificate, Protocol, TlsConnector};
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::config::SslMode;
use tokio_postgres::{AsyncMessage, Client, Config};

/// What the connection sends besides query results: notifications and notices.
pub type Messages = BoxStream<'static, Result<AsyncMessage, tokio_postgres::Error>>;

/// Open a `LISTEN` connection to `database_url`. The caller drives the
/// returned stream on its own task; when it ends, the connection is gone.
pub async fn connect(database_url: &str) -> anyhow::Result<(Client, Messages)> {
    let (config, tls) = parse(database_url)?;
    let connector = MakeTlsConnector::new(tls.connector()?);
    let (client, connection) = config.connect(connector).await?;
    let mut connection = Box::pin(connection);
    let messages = futures::stream::poll_fn(move |cx| connection.as_mut().poll_message(cx));
    Ok((client, messages.boxed()))
}

/// How much of the server's certificate is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verify {
    Nothing,
    Chain,
    ChainAndHost,
}

/// Where trusted roots come from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Roots {
    System,
    File(PathBuf),
}

/// The TLS half of a database URL, which tokio-postgres doesn't read itself:
/// it knows only `disable`, `prefer` and `require`, and no certificate options.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tls {
    mode: SslMode,
    verify: Verify,
    roots: Roots,
}

impl Tls {
    fn from_options(sslmode: Option<&str>, sslrootcert: Option<&str>) -> anyhow::Result<Self> {
        let roots = match sslrootcert {
            None | Some("system") => Roots::System,
            Some(path) => Roots::File(PathBuf::from(path)),
        };
        let (mode, verify) = match sslmode.unwrap_or("prefer") {
            "disable" => (SslMode::Disable, Verify::Nothing),
            "allow" | "prefer" => (SslMode::Prefer, Verify::Nothing),
            // libpq checks the chain under `require` when a root is given.
            "require" if sslrootcert.is_some() => (SslMode::Require, Verify::Chain),
            "require" => (SslMode::Require, Verify::Nothing),
            "verify-ca" => (SslMode::Require, Verify::Chain),
            "verify-full" => (SslMode::Require, Verify::ChainAndHost),
            other => bail!("database URL has an unknown sslmode: {other}"),
        };
        Ok(Self {
            mode,
            verify,
            roots,
        })
    }

    fn connector(&self) -> anyhow::Result<TlsConnector> {
        let mut builder = TlsConnector::builder();
        // libpq's default floor.
        builder.min_protocol_version(Some(Protocol::Tlsv12));
        match self.verify {
            Verify::Nothing => {
                builder.danger_accept_invalid_certs(true);
                builder.danger_accept_invalid_hostnames(true);
            }
            Verify::Chain => {
                builder.danger_accept_invalid_hostnames(true);
            }
            Verify::ChainAndHost => {}
        }
        if self.verify != Verify::Nothing {
            if let Roots::File(path) = &self.roots {
                let pem = std::fs::read(path)
                    .with_context(|| format!("reading sslrootcert {}", path.display()))?;
                let roots = pem_certificates(&pem)
                    .with_context(|| format!("reading sslrootcert {}", path.display()))?;
                builder.disable_built_in_roots(true);
                for root in roots {
                    builder.add_root_certificate(root);
                }
            }
        }
        builder
            .build()
            .context("building the database TLS connector")
    }
}

/// Every certificate in a PEM file.
fn pem_certificates(pem: &[u8]) -> anyhow::Result<Vec<Certificate>> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let text = std::str::from_utf8(pem).context("not a PEM file")?;
    let mut certs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let Some(len) = rest[start..].find(END) else {
            bail!("a certificate has no end line");
        };
        let block = &rest[start..start + len + END.len()];
        certs.push(Certificate::from_pem(block.as_bytes())?);
        rest = &rest[start + len + END.len()..];
    }
    if certs.is_empty() {
        bail!("no certificate in it");
    }
    Ok(certs)
}

/// libpq options with no effect here: SNI is always sent, compression is gone
/// from Postgres, and GSS encryption isn't offered by tokio-postgres.
const IGNORED_KEYS: [&str; 3] = ["sslsni", "sslcompression", "gssencmode"];
/// libpq options this connection can't honour. Refused rather than dropped,
/// so a setup that depends on one fails loudly.
const UNSUPPORTED_KEYS: [&str; 7] = [
    "sslcert",
    "sslkey",
    "sslpassword",
    "sslcrl",
    "sslcrldir",
    "ssl_min_protocol_version",
    "ssl_max_protocol_version",
];

/// Split `database_url` into the tokio-postgres config and its TLS settings.
/// Takes the URL form (`postgres://...?sslmode=require`) and the key/value
/// form (`host=... sslmode=require`); in the key/value form a TLS option's
/// value can't contain spaces.
fn parse(database_url: &str) -> anyhow::Result<(Config, Tls)> {
    let mut sslmode = None;
    let mut sslrootcert = None;
    let mut take = |key: &str, value: String| -> anyhow::Result<bool> {
        match key {
            "sslmode" => sslmode = Some(value),
            "sslrootcert" => sslrootcert = Some(value),
            k if IGNORED_KEYS.contains(&k) => {}
            k if UNSUPPORTED_KEYS.contains(&k) => {
                bail!("database URL option {k} isn't supported on the LISTEN connections")
            }
            _ => return Ok(false),
        }
        Ok(true)
    };

    let rest =
        if database_url.starts_with("postgres://") || database_url.starts_with("postgresql://") {
            match database_url.split_once('?') {
                None => database_url.to_string(),
                Some((base, query)) => {
                    let mut kept = Vec::new();
                    for pair in query.split('&').filter(|p| !p.is_empty()) {
                        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                        let key = urlencoding::decode(k).context("database URL query")?;
                        let value = urlencoding::decode(v).context("database URL query")?;
                        if !take(&key, value.into_owned())? {
                            kept.push(pair);
                        }
                    }
                    if kept.is_empty() {
                        base.to_string()
                    } else {
                        format!("{base}?{}", kept.join("&"))
                    }
                }
            }
        } else {
            let mut kept = Vec::new();
            for token in database_url.split_whitespace() {
                let consumed = match token.split_once('=') {
                    Some((k, v)) => take(k, v.trim_matches('\'').to_string())?,
                    None => false,
                };
                if !consumed {
                    kept.push(token);
                }
            }
            kept.join(" ")
        };

    let mut config: Config = rest.parse().context("parsing the database URL")?;
    let tls = Tls::from_options(sslmode.as_deref(), sslrootcert.as_deref())?;
    config.ssl_mode(tls.mode);
    Ok((config, tls))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tls(url: &str) -> Tls {
        parse(url).expect("parses").1
    }

    #[test]
    fn sslmode_chooses_tls_and_checks_as_libpq_does() {
        let base = "postgres://u:p@db.example:5432/nosdesk";
        let cases = [
            ("", SslMode::Prefer, Verify::Nothing),
            ("?sslmode=disable", SslMode::Disable, Verify::Nothing),
            ("?sslmode=allow", SslMode::Prefer, Verify::Nothing),
            ("?sslmode=prefer", SslMode::Prefer, Verify::Nothing),
            ("?sslmode=require", SslMode::Require, Verify::Nothing),
            ("?sslmode=verify-ca", SslMode::Require, Verify::Chain),
            (
                "?sslmode=verify-full",
                SslMode::Require,
                Verify::ChainAndHost,
            ),
            (
                "?sslmode=require&sslrootcert=/etc/ca.pem",
                SslMode::Require,
                Verify::Chain,
            ),
        ];
        for (query, mode, verify) in cases {
            let (config, tls) = parse(&format!("{base}{query}")).expect(query);
            assert_eq!((tls.mode, tls.verify), (mode, verify), "{query}");
            assert_eq!(config.get_ssl_mode(), mode, "{query}");
        }
    }

    #[test]
    fn sslrootcert_names_a_file_or_the_system_roots() {
        let base = "postgres://u@db/nosdesk?sslmode=verify-full";
        assert_eq!(tls(base).roots, Roots::System);
        assert_eq!(
            tls(&format!("{base}&sslrootcert=system")).roots,
            Roots::System
        );
        assert_eq!(
            tls(&format!("{base}&sslrootcert=%2Fetc%2Fssl%2Fca.pem")).roots,
            Roots::File(PathBuf::from("/etc/ssl/ca.pem"))
        );
    }

    #[test]
    fn the_rest_of_the_url_reaches_tokio_postgres() {
        let (config, _) = parse(
            "postgres://nosdesk_app:p%40ss@db.example:6543/nosdesk?sslmode=verify-full&sslrootcert=/ca.pem&application_name=nosdesk&sslsni=1",
        )
        .expect("parses");
        assert_eq!(config.get_user(), Some("nosdesk_app"));
        assert_eq!(config.get_password(), Some(&b"p@ss"[..]));
        assert_eq!(config.get_dbname(), Some("nosdesk"));
        assert_eq!(config.get_ports(), &[6543]);
        assert_eq!(config.get_application_name(), Some("nosdesk"));
    }

    #[test]
    fn the_key_value_form_works_too() {
        let (config, tls) = parse(
            "host=db.example user=nosdesk dbname=nosdesk sslmode=verify-ca sslrootcert='/ca.pem'",
        )
        .expect("parses");
        assert_eq!(tls.verify, Verify::Chain);
        assert_eq!(tls.roots, Roots::File(PathBuf::from("/ca.pem")));
        assert_eq!(config.get_ssl_mode(), SslMode::Require);
        assert_eq!(config.get_dbname(), Some("nosdesk"));
    }

    #[test]
    fn an_unknown_sslmode_or_a_client_certificate_is_refused() {
        assert!(parse("postgres://u@db/n?sslmode=sometimes").is_err());
        let err = parse("postgres://u@db/n?sslmode=require&sslcert=/c.pem&sslkey=/k.pem")
            .expect_err("client certificates aren't supported");
        assert!(format!("{err:#}").contains("sslcert"), "{err:#}");
    }

    #[test]
    fn a_root_file_without_a_certificate_is_refused() {
        assert!(pem_certificates(b"not a certificate").is_err());
        assert!(pem_certificates(b"-----BEGIN CERTIFICATE-----\nAAAA").is_err());
    }
}
