//! The LISTEN connections (live updates, the email queue, the notification
//! outbox, the search replicator) speak TLS the way the URL's `sslmode` asks,
//! as the libpq pool does. A database that only accepts TLS, such as a managed
//! Postgres, must not leave them retrying a plaintext connection forever.
//!
//! The server here is a stand-in for Postgres: it answers the SSL request,
//! reads the startup message and lets the client in without a password, which
//! is all `connect` needs. It reports whether the session was encrypted.

#![allow(clippy::expect_used)]

use std::io::Write as _;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const SSL_REQUEST: i32 = 80_877_103;
const PROTOCOL_3: i32 = 196_608;

/// How the client's session reached the server.
#[derive(Debug, PartialEq, Eq)]
enum Session {
    /// Asked for TLS and got it.
    Tls,
    /// Asked for TLS, was told no, and carried on in plaintext.
    PlainAfterRefusal,
    /// Never asked for TLS.
    Plain,
}

/// A test CA and a server certificate it signed for `server_name`.
struct Certs {
    ca_pem: String,
    server: native_tls::Identity,
}

/// Valid from yesterday for 30 days: short enough for every platform's
/// TLS server certificate rules.
fn validity(params: &mut CertificateParams) {
    use chrono::Datelike;
    let from = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let to = from + chrono::Duration::days(30);
    params.not_before = rcgen::date_time_ymd(from.year(), from.month() as u8, from.day() as u8);
    params.not_after = rcgen::date_time_ymd(to.year(), to.month() as u8, to.day() as u8);
}

/// RSA, because macOS imports a PKCS#8 RSA key for the server identity but
/// not an EC one.
fn rsa_key() -> KeyPair {
    KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).expect("rsa key")
}

fn certs(server_name: &str) -> Certs {
    let ca_key = rsa_key();
    let mut ca = CertificateParams::new(Vec::<String>::new()).expect("ca params");
    ca.distinguished_name
        .push(DnType::CommonName, "Nosdesk test CA");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    validity(&mut ca);
    let ca = ca.self_signed(&ca_key).expect("ca cert");

    let key = rsa_key();
    let mut params = CertificateParams::new(vec![server_name.to_string()]).expect("params");
    params
        .distinguished_name
        .push(DnType::CommonName, server_name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    validity(&mut params);
    let cert = params.signed_by(&key, &ca, &ca_key).expect("server cert");

    let server =
        native_tls::Identity::from_pkcs8(cert.pem().as_bytes(), key.serialize_pem().as_bytes())
            .expect("server identity");
    Certs {
        ca_pem: ca.pem(),
        server,
    }
}

/// A file holding `pem`, for `sslrootcert`.
fn root_file(pem: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(pem.as_bytes()).expect("write root");
    file
}

async fn read_i32<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<i32> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await?;
    Ok(i32::from_be_bytes(buf))
}

/// Read the rest of a startup message whose length is already read, then let
/// the client in (AuthenticationOk, ReadyForQuery) and hold the session open
/// until the client hangs up.
async fn admit<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, len: i32) -> std::io::Result<()> {
    let mut rest = vec![0u8; usize::try_from(len - 4).expect("length")];
    stream.read_exact(&mut rest).await?;
    stream.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 0]).await?;
    stream.write_all(&[b'Z', 0, 0, 0, 5, b'I']).await?;
    stream.flush().await?;
    let mut sink = [0u8; 256];
    while stream.read(&mut sink).await? > 0 {}
    Ok(())
}

/// Serve one client. With `tls`, an SSL request gets `S` and a handshake with
/// that identity; without, it gets `N`.
async fn stand_in_postgres(
    tls: Option<native_tls::Identity>,
) -> (u16, JoinHandle<std::io::Result<Session>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let len = read_i32(&mut stream).await?;
        let code = read_i32(&mut stream).await?;
        if code != SSL_REQUEST {
            assert_eq!(code, PROTOCOL_3, "a startup message");
            let mut rest = vec![0u8; usize::try_from(len - 8).expect("length")];
            stream.read_exact(&mut rest).await?;
            stream.write_all(&[b'R', 0, 0, 0, 8, 0, 0, 0, 0]).await?;
            stream.write_all(&[b'Z', 0, 0, 0, 5, b'I']).await?;
            let mut sink = [0u8; 256];
            while stream.read(&mut sink).await? > 0 {}
            return Ok(Session::Plain);
        }
        match tls {
            Some(identity) => {
                stream.write_all(b"S").await?;
                let acceptor = tokio_native_tls::TlsAcceptor::from(
                    native_tls::TlsAcceptor::new(identity).expect("acceptor"),
                );
                let mut stream = acceptor
                    .accept(stream)
                    .await
                    .map_err(std::io::Error::other)?;
                let len = read_i32(&mut stream).await?;
                admit(&mut stream, len).await?;
                Ok(Session::Tls)
            }
            None => {
                stream.write_all(b"N").await?;
                let len = read_i32(&mut stream).await?;
                admit(&mut stream, len).await?;
                Ok(Session::PlainAfterRefusal)
            }
        }
    });
    (port, handle)
}

/// A URL for the stand-in on `port`, as `localhost` (so a certificate for
/// `localhost` matches) but dialled at 127.0.0.1, with `query` appended.
fn url(port: u16, query: &str) -> String {
    let sep = if query.is_empty() { "" } else { "&" };
    format!("postgres://nosdesk@localhost:{port}/nosdesk?hostaddr=127.0.0.1{sep}{query}")
}

/// Connect, hang up, and say how the session went.
async fn session(
    port: u16,
    server: JoinHandle<std::io::Result<Session>>,
    query: &str,
) -> Result<Session, String> {
    let connected = backend::db::listen::connect(&url(port, query)).await;
    match connected {
        Ok((client, messages)) => {
            drop(client);
            drop(messages);
            let session = tokio::time::timeout(std::time::Duration::from_secs(10), server)
                .await
                .expect("server finished")
                .expect("server task")
                .expect("server session");
            Ok(session)
        }
        Err(e) => {
            server.abort();
            Err(format!("{e:#}"))
        }
    }
}

#[tokio::test]
async fn sslmode_require_connects_over_tls_without_checking_the_certificate() {
    let certs = certs("localhost");
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    assert_eq!(
        session(port, server, "sslmode=require").await,
        Ok(Session::Tls)
    );
}

#[tokio::test]
async fn no_sslmode_prefers_tls() {
    let certs = certs("localhost");
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    assert_eq!(session(port, server, "").await, Ok(Session::Tls));
}

#[tokio::test]
async fn sslmode_prefer_carries_on_in_plaintext_when_the_server_has_no_tls() {
    let (port, server) = stand_in_postgres(None).await;
    assert_eq!(
        session(port, server, "sslmode=prefer").await,
        Ok(Session::PlainAfterRefusal)
    );
}

#[tokio::test]
async fn sslmode_disable_never_asks_for_tls() {
    let certs = certs("localhost");
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    assert_eq!(
        session(port, server, "sslmode=disable").await,
        Ok(Session::Plain)
    );
}

#[tokio::test]
async fn sslmode_require_refuses_a_server_without_tls() {
    let (port, server) = stand_in_postgres(None).await;
    assert!(session(port, server, "sslmode=require").await.is_err());
}

#[tokio::test]
async fn sslmode_verify_full_trusts_the_given_root_and_checks_the_host() {
    let certs = certs("localhost");
    let root = root_file(&certs.ca_pem);
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    let query = format!("sslmode=verify-full&sslrootcert={}", root.path().display());
    assert_eq!(session(port, server, &query).await, Ok(Session::Tls));
}

#[tokio::test]
async fn sslmode_verify_full_refuses_a_certificate_from_an_unknown_root() {
    let certs = certs("localhost");
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    assert!(
        session(port, server, "sslmode=verify-full").await.is_err(),
        "the test CA isn't a system root"
    );
}

#[tokio::test]
async fn sslmode_verify_full_refuses_a_certificate_for_another_host() {
    let certs = certs("db.internal");
    let root = root_file(&certs.ca_pem);
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    let query = format!("sslmode=verify-full&sslrootcert={}", root.path().display());
    assert!(session(port, server, &query).await.is_err());
}

#[tokio::test]
async fn sslmode_verify_ca_checks_the_root_but_not_the_host() {
    let certs = certs("db.internal");
    let root = root_file(&certs.ca_pem);
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    let query = format!("sslmode=verify-ca&sslrootcert={}", root.path().display());
    assert_eq!(session(port, server, &query).await, Ok(Session::Tls));
}

#[tokio::test]
async fn sslmode_require_with_a_root_verifies_the_chain() {
    // libpq treats `require` with a root certificate as `verify-ca`.
    let certs = certs("localhost");
    let other = self::certs("localhost");
    let root = root_file(&other.ca_pem);
    let (port, server) = stand_in_postgres(Some(certs.server)).await;
    let query = format!("sslmode=require&sslrootcert={}", root.path().display());
    assert!(session(port, server, &query).await.is_err());
}

/// Against a real Postgres that accepts only TLS. Set
/// `NOSDESK_TLS_TEST_DATABASE_URL` (with `sslmode=require` or stronger) to run.
#[tokio::test]
#[ignore = "needs a TLS-only Postgres in NOSDESK_TLS_TEST_DATABASE_URL"]
async fn listens_on_a_tls_only_postgres() {
    let Ok(url) = std::env::var("NOSDESK_TLS_TEST_DATABASE_URL") else {
        eprintln!("NOSDESK_TLS_TEST_DATABASE_URL is unset; skipping");
        return;
    };
    let (client, mut messages) = backend::db::listen::connect(&url)
        .await
        .expect("connect over TLS");
    // Keep driving the connection after the notification, so the client's
    // queries still get their answers.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);
    tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(msg) = messages.next().await {
            if let Ok(tokio_postgres::AsyncMessage::Notification(n)) = msg {
                let _ = tx.send(n.payload().to_string()).await;
            }
        }
    });
    client
        .batch_execute("LISTEN nosdesk_tls_probe; NOTIFY nosdesk_tls_probe, 'over tls'")
        .await
        .expect("listen and notify");
    let got = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("notification arrives");
    assert_eq!(got.as_deref(), Some("over tls"));
    let ssl: bool = client
        .query_one(
            "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .expect("pg_stat_ssl")
        .get(0);
    assert!(ssl, "the session is encrypted");
}
