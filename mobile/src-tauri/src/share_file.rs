//! Staging a file for the native share sheet.
//!
//! The share plugin (`../tauri-plugin-share`) hands the OS a path, so a file
//! first lands in `<app cache>/share/`. That is the only folder the Android
//! `FileProvider` exposes, and on Android `app_cache_dir()` is the
//! `Context.cacheDir` its `<cache-path>` resolves against. A file gets there
//! one of two ways:
//!
//! - [`write_share_file`]: bytes the webview already holds (a PDF it rendered,
//!   a report it built). On iOS they arrive as the raw invoke body
//!   (`InvokeBody::Raw`). Android's IPC has no request body and posts the
//!   message as JSON, where a `Uint8Array` would become an array of numbers,
//!   four or more bytes per byte of file; there the JS sends `{ base64 }`
//!   instead, a third larger. The file name rides in a header, percent-encoded
//!   because header values must be ASCII.
//! - [`stage_server_file`]: a file on the server (an attachment, a backup).
//!   Rust fetches it with the session the asset proxy holds and streams it to
//!   disk, so a large file never passes through the webview's memory or IPC.
//!
//! Each staging empties the folder first, so it holds one file at a time; the
//! JS side runs one save at a time to match (`frontend/src/platform/files.ts`).
//! On Android the chooser reports done as it opens, so a quick second save can
//! remove a file the chosen app has yet to read.

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Serialize;
use tauri::http::header;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, Runtime};
use tokio::io::AsyncWriteExt as _;

use crate::asset_proxy::{self, AssetProxy, Upstream};

/// Header carrying the percent-encoded file name (`encodeURIComponent`).
pub const FILENAME_HEADER: &str = "x-share-filename";

const FALLBACK_NAME: &str = "file";
const MAX_NAME_CHARS: usize = 120;
const FALLBACK_MIME: &str = "application/octet-stream";

/// Writes the invoke body to `<app cache>/share/<sanitised name>` and returns
/// the absolute path.
#[tauri::command]
pub async fn write_share_file<R: Runtime>(
    app: AppHandle<R>,
    request: Request<'_>,
) -> Result<String, String> {
    let raw_name = request
        .headers()
        .get(FILENAME_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_default();
    let name = sanitize_filename(&raw_name);
    let bytes = body_bytes(request.body())?;
    let dir = share_dir(&app)?;

    let path = tauri::async_runtime::spawn_blocking(move || write_fresh(&dir, &name, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("could not write the file: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// A file staged for the share sheet, and the type to offer it as.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedFile {
    pub path: String,
    pub mime_type: String,
}

/// Downloads the server file at `path` (`/api/...` or `/uploads/...`) into
/// `<app cache>/share/` with the session's bearer. The file is named
/// `filename` when given, else as the server's `Content-Disposition` names
/// it, else after the path's last segment.
#[tauri::command]
pub async fn stage_server_file<R: Runtime>(
    app: AppHandle<R>,
    proxy: tauri::State<'_, AssetProxy>,
    path: String,
    filename: Option<String>,
) -> Result<StagedFile, String> {
    let (token, base_url) = proxy.read();
    let upstream = server_file_upstream(token.as_deref(), base_url.as_deref(), &path)?;
    let dir = share_dir(&app)?;
    download(proxy.client(), upstream, &dir, filename.as_deref()).await
}

/// The GET for a server file. Unlike a webview load through the proxy scheme,
/// `path` here is any string the page passed, so the URL it makes must stay on
/// the configured server and on a file route: the bearer goes nowhere else.
fn server_file_upstream(
    token: Option<&str>,
    base_url: Option<&str>,
    path: &str,
) -> Result<Upstream, String> {
    let upstream = asset_proxy::upstream(token, base_url, path, None)
        .ok_or_else(|| "no server is configured".to_string())?;
    let base = reqwest::Url::parse(base_url.unwrap_or_default())
        .map_err(|_| "the server address is not a URL".to_string())?;
    let url = reqwest::Url::parse(&upstream.url).map_err(|_| "not a file path".to_string())?;
    let on_server = url.origin() == base.origin();
    // The path as given, with nothing for the URL parser to resolve: no `..`
    // or encoded dot segments that would land the request somewhere else.
    // Compared decoded, since the parser percent-encodes a space or accent.
    let given = path.split(['?', '#']).next().unwrap_or_default();
    let as_given = percent_decode(given) == percent_decode(url.path());
    let on_file_route = url.path().starts_with("/api/") || url.path().starts_with("/uploads/");
    if !path.starts_with('/') || !on_server || !as_given || !on_file_route {
        return Err("not a file on the server".to_string());
    }
    Ok(upstream)
}

/// Fetches `upstream` and streams the body to a fresh file in `dir`.
async fn download(
    client: &reqwest::Client,
    upstream: Upstream,
    dir: &Path,
    filename: Option<&str>,
) -> Result<StagedFile, String> {
    let mut req = client.get(&upstream.url);
    for (name, value) in &upstream.headers {
        req = req.header(name, value);
    }
    let mut resp = req
        .send()
        .await
        .map_err(|e| format!("could not reach the server: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("the server answered {}", resp.status()));
    }

    let headers = resp.headers();
    let name = filename
        .filter(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            headers
                .get(header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok())
                .and_then(disposition_filename)
        })
        .unwrap_or_else(|| last_segment(resp.url().path()));
    let name = sanitize_filename(&name);
    let mime_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(FALLBACK_MIME)
        .to_owned();

    let path = {
        let dir = dir.to_owned();
        tauri::async_runtime::spawn_blocking(move || fresh_path(&dir, &name))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("could not prepare the file: {e}"))?
    };
    let written = async {
        let mut file = tokio::fs::File::create(&path).await?;
        while let Some(chunk) = resp.chunk().await.map_err(io::Error::other)? {
            file.write_all(&chunk).await?;
        }
        file.flush().await
    }
    .await;
    if let Err(e) = written {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(format!("could not download the file: {e}"));
    }

    Ok(StagedFile {
        path: path.to_string_lossy().into_owned(),
        mime_type,
    })
}

fn share_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache directory: {e}"))?
        .join("share"))
}

/// The file bytes from either IPC shape (see the module doc).
fn body_bytes(body: &InvokeBody) -> Result<Vec<u8>, String> {
    match body {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(value) => {
            let encoded = value
                .get("base64")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "file body is neither bytes nor { base64 }".to_string())?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| format!("file body is not valid base64: {e}"))
        }
    }
}

/// Empties `dir` and returns where `name` goes in it.
fn fresh_path(dir: &Path, name: &str) -> io::Result<PathBuf> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::fs::create_dir_all(dir)?;
    Ok(dir.join(name))
}

/// Empties `dir`, then writes `bytes` to `dir/name`.
fn write_fresh(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = fresh_path(dir, name)?;
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

/// The file name a `Content-Disposition` value gives: the exact
/// `filename*=UTF-8''...` (RFC 6266) when present, else the plain `filename`,
/// quoted or not. The server sends the plain form, and adds `filename*` when
/// the plain one had to lose characters.
fn disposition_filename(value: &str) -> Option<String> {
    let params = || value.split(';').skip(1).filter_map(|p| p.split_once('='));
    let extended = params()
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("filename*"))
        .and_then(|(_, v)| {
            let (charset, rest) = v.trim().split_once('\'')?;
            let (_language, encoded) = rest.split_once('\'')?;
            charset
                .eq_ignore_ascii_case("utf-8")
                .then(|| percent_decode(encoded))
        });
    let plain = || {
        params()
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("filename"))
            .map(|(_, v)| v.trim().trim_matches('"').replace("\\\"", "\""))
    };
    extended.or_else(plain).filter(|n| !n.is_empty())
}

/// The last segment of a URL path, percent-decoded.
fn last_segment(path: &str) -> String {
    percent_decode(path.rsplit('/').next().unwrap_or_default())
}

/// A file name that is safe to join onto the share folder: the last path
/// component only, without control characters or `/ \ : * ? " < > |`, no
/// leading dots, at most 120 characters with the extension kept. Anything
/// that ends up empty becomes `file`.
fn sanitize_filename(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() {
        return FALLBACK_NAME.to_string();
    }
    if cleaned.chars().count() <= MAX_NAME_CHARS {
        return cleaned.to_string();
    }

    // Keep a short extension and cut the stem so the whole name fits.
    let (stem, ext) = match cleaned.rfind('.') {
        Some(i) if i > 0 && cleaned[i..].chars().count() <= 16 => cleaned.split_at(i),
        _ => (cleaned, ""),
    };
    let keep = MAX_NAME_CHARS - ext.chars().count();
    let stem: String = stem.chars().take(keep).collect();
    format!("{}{}", stem.trim_end(), ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_kept() {
        assert_eq!(sanitize_filename("Invoice 42.pdf"), "Invoice 42.pdf");
        assert_eq!(sanitize_filename("Résumé été.pdf"), "Résumé été.pdf");
    }

    #[test]
    fn only_the_last_path_component_survives() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Users\\me\\report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("a/b/"), FALLBACK_NAME);
    }

    #[test]
    fn reserved_and_control_characters_are_stripped() {
        assert_eq!(sanitize_filename("a:b*c?d\"e<f>g|h.pdf"), "abcdefgh.pdf");
        assert_eq!(sanitize_filename("line\nbreak\t.pdf"), "linebreak.pdf");
    }

    #[test]
    fn dot_names_and_empty_names_fall_back() {
        assert_eq!(sanitize_filename(""), FALLBACK_NAME);
        assert_eq!(sanitize_filename(".."), FALLBACK_NAME);
        assert_eq!(sanitize_filename("."), FALLBACK_NAME);
        assert_eq!(sanitize_filename("  ???  "), FALLBACK_NAME);
        assert_eq!(sanitize_filename(".hidden.pdf"), "hidden.pdf");
    }

    #[test]
    fn a_long_name_is_cut_and_keeps_its_extension() {
        let long = format!("{}.pdf", "é".repeat(300));
        let out = sanitize_filename(&long);
        assert_eq!(out.chars().count(), MAX_NAME_CHARS);
        assert!(out.ends_with(".pdf"));

        let no_ext = "x".repeat(300);
        assert_eq!(sanitize_filename(&no_ext).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn both_body_shapes_give_the_bytes() {
        assert_eq!(
            body_bytes(&InvokeBody::Raw(vec![37, 80, 68, 70])),
            Ok(vec![37, 80, 68, 70])
        );
        assert_eq!(
            body_bytes(&InvokeBody::Json(
                serde_json::json!({ "base64": "JVBERg==" })
            )),
            Ok(vec![37, 80, 68, 70])
        );
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!({ "base64": "%%" }))).is_err());
        // A number array (what a bare Uint8Array becomes on Android) is refused,
        // so the 4x path can't come back unnoticed.
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!([37, 80]))).is_err());
    }

    #[test]
    fn a_server_file_stays_on_the_server_and_its_file_routes() {
        let base = Some("https://help.example");
        let ok = server_file_upstream(Some("tok"), base, "/api/files/tickets/1/a.wav").unwrap();
        assert_eq!(ok.url, "https://help.example/api/files/tickets/1/a.wav");
        assert_eq!(
            ok.headers,
            vec![(header::AUTHORIZATION, "Bearer tok".to_string())]
        );
        assert!(server_file_upstream(None, base, "/uploads/branding/logo.png").is_ok());
        assert!(server_file_upstream(None, base, "/api/files/tickets/1/voice note é.wav").is_ok());

        for refused in [
            "//evil.example/api/files/x",
            "@evil.example/api/files/x",
            "https://evil.example/api/files/x",
            "api/files/x",
            "/api/../auth/logout",
            "/api/%2e%2e/%2e%2e/admin",
            "/uploads/../api/auth/logout",
            "/api/files/%2e%2e/%2e%2e/auth/me",
            "/assets/app.js",
            "",
        ] {
            assert!(
                server_file_upstream(Some("tok"), base, refused).is_err(),
                "{refused} should be refused"
            );
        }
        assert!(server_file_upstream(Some("tok"), None, "/api/files/x").is_err());
    }

    #[test]
    fn the_disposition_names_the_file() {
        assert_eq!(
            disposition_filename("attachment; filename=\"backup-2026.zip\""),
            Some("backup-2026.zip".to_string())
        );
        assert_eq!(
            disposition_filename("inline; filename=notes.txt"),
            Some("notes.txt".to_string())
        );
        // The exact name wins over the plain one the server had to flatten.
        assert_eq!(
            disposition_filename(
                "inline; filename=\"R_sum_.pdf\"; filename*=UTF-8''R%C3%A9sum%C3%A9.pdf"
            ),
            Some("Résumé.pdf".to_string())
        );
        assert_eq!(disposition_filename("inline"), None);
        assert_eq!(disposition_filename("attachment; filename=\"\""), None);
    }

    /// A server on a local port that answers one request with `status`,
    /// `headers` and `body`, and reports the `Authorization` it was sent.
    fn serve_once(
        status: &'static str,
        headers: &'static str,
        body: Vec<u8>,
    ) -> (String, std::sync::mpsc::Receiver<Option<String>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut auth = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.trim_end().split_once(':') {
                    if name.eq_ignore_ascii_case("authorization") {
                        auth = Some(value.trim().to_string());
                    }
                }
            }
            let _ = tx.send(auth);
            let head = format!(
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        (base, rx)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nosdesk-share-{tag}-{}", std::process::id()))
    }

    #[test]
    fn a_server_file_streams_to_disk_under_its_name_and_type() {
        // Several reads' worth, so the body arrives in more than one chunk.
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let (base, auth) = serve_once(
            "200 OK",
            "Content-Type: audio/wav\r\nContent-Disposition: inline; filename=\"voice note.wav\"\r\n",
            body.clone(),
        );
        let dir = temp_dir("stream");
        let up = server_file_upstream(
            Some("tok"),
            Some(&base),
            "/api/files/tickets/1/abc_voice.wav",
        )
        .unwrap();
        let staged =
            tauri::async_runtime::block_on(download(&reqwest::Client::new(), up, &dir, None))
                .unwrap();

        assert_eq!(auth.recv().unwrap().as_deref(), Some("Bearer tok"));
        assert_eq!(staged.mime_type, "audio/wav");
        assert!(staged.path.ends_with("voice note.wav"));
        assert_eq!(std::fs::read(&staged.path).unwrap(), body);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_callers_name_wins_and_the_path_is_the_last_resort() {
        let (base, _) = serve_once(
            "200 OK",
            "Content-Type: text/csv; charset=utf-8\r\n",
            b"a,b".to_vec(),
        );
        let dir = temp_dir("named");
        let up = server_file_upstream(None, Some(&base), "/api/imports/7/errors.csv").unwrap();
        let staged = tauri::async_runtime::block_on(download(
            &reqwest::Client::new(),
            up,
            &dir,
            Some("Import errors.csv"),
        ))
        .unwrap();
        assert!(staged.path.ends_with("Import errors.csv"));
        assert_eq!(staged.mime_type, "text/csv");

        let (base, _) = serve_once("200 OK", "", b"x".to_vec());
        let up = server_file_upstream(None, Some(&base), "/api/files/a%20b.bin").unwrap();
        let staged =
            tauri::async_runtime::block_on(download(&reqwest::Client::new(), up, &dir, Some(" ")))
                .unwrap();
        assert!(staged.path.ends_with("a b.bin"));
        assert_eq!(staged.mime_type, FALLBACK_MIME);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refusal_is_an_error_and_leaves_no_file() {
        let (base, _) = serve_once(
            "403 Forbidden",
            "Content-Type: application/json\r\n",
            b"{}".to_vec(),
        );
        let dir = temp_dir("refused");
        let up =
            server_file_upstream(Some("tok"), Some(&base), "/api/files/tickets/1/x.pdf").unwrap();
        let err = tauri::async_runtime::block_on(download(&reqwest::Client::new(), up, &dir, None))
            .unwrap_err();
        assert!(err.contains("403"), "{err}");
        assert!(!dir.join("x.pdf").exists());
    }

    #[test]
    fn writing_replaces_older_files() {
        let dir = std::env::temp_dir().join(format!("nosdesk-share-test-{}", std::process::id()));
        let first = write_fresh(&dir, "one.pdf", b"1").unwrap();
        let second = write_fresh(&dir, "two.pdf", b"2").unwrap();
        assert!(!first.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"2");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
