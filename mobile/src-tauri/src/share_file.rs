//! Staging a file for the native share sheet.
//!
//! The share plugin (`../tauri-plugin-share`) hands the OS a path, so the bytes
//! the webview holds (a PDF it fetched through the asset proxy, say) first land
//! in `<app cache>/share/`. That is the only folder the Android `FileProvider`
//! exposes, and on Android `app_cache_dir()` is the `Context.cacheDir` its
//! `<cache-path>` resolves against.
//!
//! On iOS the bytes arrive as the raw invoke body (`InvokeBody::Raw`). Android's
//! IPC has no request body and posts the message as JSON, where a `Uint8Array`
//! would become an array of numbers, four or more bytes per byte of file; there
//! the JS sends `{ base64 }` instead, a third larger. The file name rides in a
//! header, percent-encoded because header values must be ASCII.

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, Runtime};

/// Header carrying the percent-encoded file name (`encodeURIComponent`).
pub const FILENAME_HEADER: &str = "x-share-filename";

const FALLBACK_NAME: &str = "document.pdf";
const MAX_NAME_CHARS: usize = 120;

/// Writes the invoke body to `<app cache>/share/<sanitised name>` and returns
/// the absolute path. Older files in that folder go first: each is only needed
/// until its share sheet closes.
#[tauri::command]
pub async fn write_share_file<R: Runtime>(
    app: AppHandle<R>,
    request: Request<'_>,
) -> Result<String, String> {
    let raw_name = request
        .headers()
        .get(FILENAME_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    let name = sanitize_filename(&raw_name);
    let bytes = body_bytes(request.body())?;

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache directory: {e}"))?
        .join("share");

    let path = tauri::async_runtime::spawn_blocking(move || write_fresh(&dir, &name, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("could not write the file: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
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

/// Empties `dir`, then writes `bytes` to `dir/name`.
fn write_fresh(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// A file name that is safe to join onto the share folder: the last path
/// component only, without control characters or `/ \ : * ? " < > |`, no
/// leading dots, at most 120 characters with the extension kept. Anything
/// that ends up empty becomes `document.pdf`.
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
            body_bytes(&InvokeBody::Json(serde_json::json!({ "base64": "JVBERg==" }))),
            Ok(vec![37, 80, 68, 70])
        );
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!({ "base64": "%%" }))).is_err());
        // A number array (what a bare Uint8Array becomes on Android) is refused,
        // so the 4x path can't come back unnoticed.
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!([37, 80]))).is_err());
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
