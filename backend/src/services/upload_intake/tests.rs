use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};

use actix_web::error::PayloadError;
use actix_web::http::header::{self, HeaderMap, HeaderValue};
use actix_web::http::StatusCode;
use futures::stream;

use super::*;

fn area(dir: &Path, budget_bytes: u64) -> SpoolArea {
    SpoolArea::new(dir.to_path_buf(), budget_bytes)
}

fn policy(max_bytes: u64, types: UploadTypes) -> UploadPolicy {
    UploadPolicy {
        max_bytes,
        max_files: Some(3),
        types,
        folder: "temp".to_string(),
        file_field: "files",
        text_fields: &["transcription"],
    }
}

fn chunks(parts: Vec<Vec<u8>>) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
    stream::iter(parts.into_iter().map(|p| Ok(Bytes::from(p))))
}

fn spool_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

async fn response(e: IntakeError) -> (StatusCode, HeaderMap, serde_json::Value) {
    let resp = actix_web::Error::from(e).error_response();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = actix_web::body::to_bytes(resp.into_body())
        .await
        .unwrap_or_default();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

/// Bytes `infer` doesn't recognize, so the blocklist takes them.
fn plain(len: usize) -> Vec<u8> {
    (0..len).map(|i| b'a' + (i % 26) as u8).collect()
}

#[tokio::test]
async fn the_head_is_the_first_8_kib_and_the_hash_covers_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = plain(20 * 1024);
    let parts = vec![
        data[..5000].to_vec(),
        data[5000..9000].to_vec(),
        data[9000..].to_vec(),
    ];
    let file = area(dir.path(), MIB)
        .stream(
            chunks(parts),
            "notes.txt",
            &policy(MIB, UploadTypes::Blocklist),
        )
        .await
        .unwrap();
    assert_eq!(file.head(), &data[..HEAD_BYTES]);
    assert_eq!(file.size, data.len() as u64);
    assert_eq!(
        file.sha256,
        hex::encode(ring::digest::digest(&ring::digest::SHA256, &data))
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), data);
}

#[tokio::test]
async fn the_hash_of_a_chunked_stream_matches_the_known_vector() {
    let dir = tempfile::tempdir().unwrap();
    let file = area(dir.path(), MIB)
        .stream(
            chunks(vec![b"a".to_vec(), b"bc".to_vec()]),
            "abc.txt",
            &policy(MIB, UploadTypes::Blocklist),
        )
        .await
        .unwrap();
    // SHA-256("abc"), FIPS 180-2 appendix B.1.
    assert_eq!(
        file.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(file.head(), b"abc");
    assert_eq!(file.display_name, "abc.txt");
    assert_eq!(file.mime, "application/octet-stream");
}

#[tokio::test]
async fn the_spool_file_goes_when_the_upload_drops() {
    let dir = tempfile::tempdir().unwrap();
    let file = area(dir.path(), MIB)
        .stream(
            chunks(vec![plain(100)]),
            "a.txt",
            &policy(MIB, UploadTypes::Blocklist),
        )
        .await
        .unwrap();
    assert_eq!(spool_files(dir.path()), 1);
    assert!(file
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(SPOOL_PREFIX));
    drop(file);
    assert_eq!(spool_files(dir.path()), 0);
}

#[tokio::test]
async fn a_chunked_upload_stops_at_the_first_chunk_past_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = pulled.clone();
    let endless = stream::repeat_with(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok::<_, Infallible>(Bytes::from(plain(1024)))
    });
    let err = area(dir.path(), MIB)
        .stream(
            endless,
            "big.txt",
            &policy(10 * 1024, UploadTypes::Blocklist),
        )
        .await
        .unwrap_err();
    assert_eq!(pulled.load(Ordering::SeqCst), 11);
    assert_eq!(spool_files(dir.path()), 0, "the partial file is removed");
    let (status, _, body) = response(err).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "FILE_TOO_LARGE");
    assert_eq!(body["file"], "big.txt");
}

/// A Windows executable under a harmless name, `chunks` of 4 KiB long.
fn executable(chunks: usize) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
    stream::iter((0..chunks).map(|n| {
        let mut chunk = vec![0u8; 4096];
        if n == 0 {
            chunk[..2].copy_from_slice(b"MZ");
        }
        Ok(Bytes::from(chunk))
    }))
}

#[tokio::test]
async fn a_blocked_type_is_refused_from_the_head_and_never_spooled_on() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), 8 * 1024);
    // Past the head the file is read to its end but not written, so it
    // takes no more of an 8 KiB budget than its head.
    let err = area
        .stream(
            executable(64),
            "report.pdf",
            &policy(MIB, UploadTypes::Blocklist),
        )
        .await
        .unwrap_err();
    assert_eq!(spool_files(dir.path()), 0);
    let (status, _, body) = response(err).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "FILE_TYPE_NOT_ALLOWED");
    assert_eq!(body["file"], "report.pdf");

    // Over the cap it is a 413, as when the size was checked first.
    let err = area
        .stream(
            executable(64),
            "report.pdf",
            &policy(64 * 1024, UploadTypes::Blocklist),
        )
        .await
        .unwrap_err();
    let (status, _, _) = response(err).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_small_file_is_checked_when_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let err = area
        .stream(
            chunks(vec![b"#!/bin/sh\n".to_vec()]),
            "setup.sh",
            &policy(MIB, UploadTypes::Blocklist),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        IntakeError::Refused {
            reason: Refusal::Validation(FileValidationError::BlockedExtension { .. }),
            ..
        }
    ));
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let file = area
        .stream(
            chunks(vec![png.clone()]),
            "shot.png",
            &policy(MIB, UploadTypes::Images),
        )
        .await
        .unwrap();
    assert_eq!(file.mime, "image/png");
    let err = area
        .stream(
            chunks(vec![b"%PDF-1.7\n".to_vec()]),
            "doc.pdf",
            &policy(MIB, UploadTypes::Images),
        )
        .await
        .unwrap_err();
    let (status, _, body) = response(err).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Only image files are allowed");
    assert_eq!(body["code"], "FILE_TYPE_NOT_ALLOWED");
    assert_eq!(spool_files(dir.path()), 1, "only the accepted png is left");
}

#[tokio::test]
async fn the_guest_and_zip_types_check_the_head() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let file = area
        .stream(
            chunks(vec![b"%PDF-1.7\n".to_vec()]),
            "form.pdf",
            &policy(MIB, UploadTypes::GuestAllowlist),
        )
        .await
        .unwrap();
    assert_eq!(file.mime, "application/pdf");
    assert!(area
        .stream(
            chunks(vec![plain(10)]),
            "notes.csv",
            &policy(MIB, UploadTypes::GuestAllowlist),
        )
        .await
        .is_err());
    let file = area
        .stream(
            chunks(vec![b"PK\x03\x04rest".to_vec()]),
            "export.zip",
            &policy(MIB, UploadTypes::Zip),
        )
        .await
        .unwrap();
    assert_eq!(file.mime, "application/zip");
    assert!(area
        .stream(
            chunks(vec![plain(10)]),
            "export.zip",
            &policy(MIB, UploadTypes::Zip),
        )
        .await
        .is_err());
}

#[tokio::test]
async fn an_image_list_takes_only_its_types_and_any_takes_anything() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let favicon = policy(
        MIB,
        UploadTypes::ImageOf(&["image/x-icon", "image/vnd.microsoft.icon", "image/png"]),
    );
    // An ICO's header sniffs as an icon type the favicon slot lists.
    let mut ico = vec![0, 0, 1, 0, 1, 0, 16, 16];
    ico.resize(64, 0);
    let file = area
        .stream(chunks(vec![ico]), "favicon.ico", &favicon)
        .await
        .unwrap();
    assert!(
        favicon_types().contains(&file.mime.as_str()),
        "{}",
        file.mime
    );

    let gif = b"GIF89a\x01\0\x01\0".to_vec();
    match area
        .stream(chunks(vec![gif]), "favicon.gif", &favicon)
        .await
    {
        Err(IntakeError::Refused {
            reason: Refusal::UnsupportedImage { detected },
            ..
        }) => assert_eq!(detected, "image/gif"),
        other => panic!("expected an unsupported image, got {other:?}"),
    }
    match area
        .stream(chunks(vec![b"%PDF-1.7".to_vec()]), "favicon.pdf", &favicon)
        .await
    {
        Err(IntakeError::Refused {
            reason: Refusal::NotAnImage,
            ..
        }) => {}
        other => panic!("expected not an image, got {other:?}"),
    }

    // Any keeps blocked names and types: the path decides.
    let any = policy(MIB, UploadTypes::Any);
    let file = area
        .stream(chunks(vec![b"MZ\x90\0".to_vec()]), "setup.exe", &any)
        .await
        .unwrap();
    assert_eq!(file.mime, "application/vnd.microsoft.portable-executable");
    let file = area
        .stream(chunks(vec![plain(10)]), "rows.csv", &any)
        .await
        .unwrap();
    assert_eq!(file.mime, "application/octet-stream");
}

fn favicon_types() -> [&'static str; 3] {
    ["image/x-icon", "image/vnd.microsoft.icon", "image/png"]
}

#[test]
fn each_path_keeps_its_own_cap() {
    assert_eq!(
        UploadPolicy::document_images("collab/doc/x".into()).max_bytes,
        10 * MIB
    );
    assert_eq!(UploadPolicy::profile_image().max_bytes, 10 * MIB);
    assert_eq!(UploadPolicy::branding_image(&[]).max_bytes, 2 * MIB);
    assert_eq!(UploadPolicy::guest().max_bytes, 10 * MIB);
    assert_eq!(UploadPolicy::spreadsheet_import().max_bytes, 10 * MIB);
    assert_eq!(UploadPolicy::inbound_attachment(1).max_bytes, 25 * MIB);
    assert_eq!(
        UploadPolicy::asset_media(1).max_bytes,
        (25 * MIB).min(get_max_file_size() as u64)
    );
    // One file a request: a Content-Length past one at the cap is refused.
    assert_eq!(
        UploadPolicy::guest().max_request_bytes(),
        Some(10 * MIB + ENVELOPE_BYTES)
    );
}

#[tokio::test]
async fn a_broken_stream_is_reported_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let broken = stream::iter(vec![Ok(Bytes::from(plain(10))), Err("reset by peer")]);
    let err = area(dir.path(), MIB)
        .stream(broken, "a.txt", &policy(MIB, UploadTypes::Blocklist))
        .await
        .unwrap_err();
    assert!(matches!(err, IntakeError::Unreadable(ref e) if e == "reset by peer"));
    assert_eq!(spool_files(dir.path()), 0);
}

#[tokio::test]
async fn a_spent_budget_answers_503_with_retry_after_and_comes_back_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), 10 * 1024);
    let p = policy(MIB, UploadTypes::Blocklist);
    let first = area
        .stream(chunks(vec![plain(8 * 1024)]), "a.txt", &p)
        .await
        .unwrap();
    let err = area
        .stream(chunks(vec![plain(4 * 1024)]), "b.txt", &p)
        .await
        .unwrap_err();
    assert!(matches!(err, IntakeError::Busy));
    assert_eq!(spool_files(dir.path()), 1, "the refused file is removed");
    let (status, headers, _) = response(err).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(headers.contains_key(header::RETRY_AFTER));

    drop(first);
    area.stream(chunks(vec![plain(4 * 1024)]), "b.txt", &p)
        .await
        .expect("the budget is given back when a file drops");
}

#[test]
fn the_budget_defaults_to_four_files_and_never_drops_below_one() {
    assert_eq!(spool_budget_mb_from(None, 50), 200);
    assert_eq!(spool_budget_mb_from(Some("1000"), 50), 1000);
    assert_eq!(spool_budget_mb_from(Some("10"), 50), 50);
    assert_eq!(spool_budget_mb_from(Some("lots"), 200), 800);
    assert_eq!(
        spool_dir_from(None),
        std::env::temp_dir().join("nosdesk-spool")
    );
    assert_eq!(spool_dir_from(Some(" ")), spool_dir_from(None));
    assert_eq!(
        spool_dir_from(Some("/var/spool/nosdesk")),
        PathBuf::from("/var/spool/nosdesk")
    );
}

#[test]
fn the_sweep_removes_only_old_spool_files() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    let touch = |name: &str, modified: Option<SystemTime>| {
        let f = std::fs::File::create(dir.path().join(name)).unwrap();
        if let Some(t) = modified {
            f.set_modified(t).unwrap();
        }
    };
    touch("nosdesk-upload-old", Some(old));
    touch("nosdesk-upload-fresh", None);
    touch("someone-elses-file", Some(old));
    assert_eq!(area.sweep(SPOOL_MAX_AGE).unwrap(), 1);
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["nosdesk-upload-fresh", "someone-elses-file"]);

    let missing = SpoolArea::new(dir.path().join("never-made"), MIB);
    assert_eq!(missing.sweep(SPOOL_MAX_AGE).unwrap(), 0);
}

fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, data) in parts {
        body.extend_from_slice(b"--x\r\nContent-Disposition: form-data; name=\"");
        body.extend_from_slice(name.as_bytes());
        body.push(b'"');
        if let Some(f) = filename {
            body.extend_from_slice(b"; filename=\"");
            body.extend_from_slice(f.as_bytes());
            body.push(b'"');
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b"--x--\r\n");
    body
}

/// A multipart payload of `body` in 1 KiB chunks, counting how many are
/// pulled.
fn payload(body: Vec<u8>, pulled: Arc<AtomicUsize>) -> Multipart {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("multipart/form-data; boundary=x"),
    );
    let chunks: Vec<Result<Bytes, PayloadError>> = body
        .chunks(1024)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    let counted = stream::iter(chunks).inspect(move |_| {
        pulled.fetch_add(1, Ordering::SeqCst);
    });
    Multipart::new(&headers, counted)
}

#[tokio::test]
async fn a_content_length_over_the_request_cap_is_refused_before_any_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let p = policy(1024, UploadTypes::Blocklist);
    let max = p.max_request_bytes().unwrap();
    assert_eq!(max, 3 * 1024 + MIB);
    let pulled = Arc::new(AtomicUsize::new(0));
    let body = multipart_body(&[("files", Some("a.txt"), &plain(10))]);
    let err = area
        .multipart(Some(max + 1), payload(body, pulled.clone()), &p)
        .err()
        .expect("refused");
    assert_eq!(pulled.load(Ordering::SeqCst), 0, "no body chunk was pulled");
    let (status, _, body) = response(err).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "FILE_TOO_LARGE");

    // A policy with no file count has no request cap.
    let any = UploadPolicy {
        max_files: None,
        ..p.clone()
    };
    let body = multipart_body(&[("files", Some("a.txt"), &plain(10))]);
    assert!(area
        .multipart(Some(u64::MAX), payload(body, pulled), &any)
        .is_ok());
}

#[tokio::test]
async fn a_multipart_upload_yields_its_parts_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let p = policy(MIB, UploadTypes::Blocklist);
    let big = plain(3000);
    let body = multipart_body(&[
        ("transcription", None, b"hello there"),
        ("other", None, b"ignored"),
        ("files", Some("one.txt"), &big),
        ("files", Some("Two (final).txt"), b"two"),
    ]);
    let pulled = Arc::new(AtomicUsize::new(0));
    let mut upload = area
        .multipart(Some(body.len() as u64), payload(body, pulled), &p)
        .unwrap();
    let mut seen = Vec::new();
    while let Some(part) = upload.next().await.unwrap() {
        match part {
            Part::Text { name, value } => seen.push(format!("{name}={value}")),
            Part::File(f) => {
                assert_eq!(spool_files(dir.path()), 1, "one file spooled at a time");
                seen.push(format!(
                    "{}:{}:{}",
                    f.display_name, f.sanitized_name, f.size
                ));
            }
        }
    }
    assert_eq!(
        seen,
        [
            "transcription=hello there",
            "one.txt:one.txt:3000",
            "Two (final).txt:Twofinal.txt:3",
        ]
    );
    assert_eq!(spool_files(dir.path()), 0);
}

#[tokio::test]
async fn a_multipart_upload_refuses_extra_files_missing_names_and_long_text() {
    let dir = tempfile::tempdir().unwrap();
    let area = area(dir.path(), MIB);
    let p = UploadPolicy {
        max_files: Some(1),
        ..policy(MIB, UploadTypes::Blocklist)
    };
    let drain = |body: Vec<u8>| {
        let area = &area;
        let p = &p;
        async move {
            let mut upload = area
                .multipart(None, payload(body, Arc::default()), p)
                .unwrap();
            loop {
                match upload.next().await {
                    Ok(Some(_)) => continue,
                    Ok(None) => return None,
                    Err(e) => return Some(e),
                }
            }
        }
    };

    let err = drain(multipart_body(&[
        ("files", Some("a.txt"), b"a"),
        ("files", Some("b.txt"), b"b"),
    ]))
    .await;
    assert!(matches!(err, Some(IntakeError::TooManyFiles { max: 1 })));

    let err = drain(multipart_body(&[("files", None, b"a")])).await;
    let (status, _, body) = response(err.expect("refused")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Filename is required");

    let long = vec![b'a'; MAX_TEXT_FIELD_BYTES + 1];
    let err = drain(multipart_body(&[("transcription", None, &long)])).await;
    let (status, _, body) = response(err.expect("refused")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Transcription too large (max 64KB)");
    assert_eq!(spool_files(dir.path()), 0);
}
