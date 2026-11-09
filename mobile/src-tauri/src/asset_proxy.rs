//! Authenticated asset proxy for the webview.
//!
//! The webview can't load workspace-scoped files directly: a relative URL
//! resolves against the `tauri://localhost` origin, and a direct `<img>` /
//! `<audio>` / `<video>` load carries no bearer (the auth interceptor only
//! applies to the native HTTP plugin, not webview resource loads). So file URLs
//! are rewritten to the `nosdesk-asset` scheme and this Rust handler proxies
//! each request to the real API with the session bearer, forwarding `Range` so
//! media seeks natively. The backend resolves the workspace from the resource,
//! so the proxied request only needs the bearer.
//!
//! The body is a full buffer per response: wry has no streaming body type. So
//! the proxy bounds what one ranged response holds. A range that is open-ended
//! (`bytes=N-`) or wider than [`RANGE_CHUNK_BYTES`] goes upstream as one chunk
//! from its start, and the server's 206 names the bytes it sent. A media player
//! reads that `Content-Range` and asks for the rest. Bounded ranges are clamped
//! too, not only open-ended ones, because iOS's media stack can ask for a range
//! that runs to the end of the file (`bytes=0-<len-1>`) rather than `bytes=0-`.

use std::borrow::Cow;
use std::sync::{Arc, RwLock};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

/// Custom scheme the webview uses for authenticated file URLs. iOS sees
/// `nosdesk-asset://localhost/<path>`; Android rewrites it to
/// `http://nosdesk-asset.localhost/<path>`. Either way the request reaches this
/// handler and `uri().path_and_query()` carries the API path.
pub const SCHEME: &str = "nosdesk-asset";

/// The most a proxied range response carries, 4 MiB. The webview's media
/// element keeps a few requests in flight, so this keeps the proxy's buffers in
/// the low tens of MB on a phone. A 100 MB video is about 25 requests, and a
/// seek waits for one chunk before playback resumes.
const RANGE_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// Session the handler reads on each request, updated from JS
/// (`set_asset_proxy_session`) on login / refresh / logout.
#[derive(Clone)]
pub struct AssetProxy {
    session: Arc<RwLock<Session>>,
    client: reqwest::Client,
}

#[derive(Default)]
struct Session {
    token: Option<String>,
    base_url: Option<String>,
}

impl AssetProxy {
    pub fn new() -> Self {
        Self {
            session: Arc::new(RwLock::new(Session::default())),
            client: reqwest::Client::new(),
        }
    }

    fn set(&self, token: Option<String>, base_url: Option<String>) {
        let mut s = self.session.write().unwrap();
        s.token = token;
        // Keep the last base if the caller didn't supply one (token-only refresh).
        if base_url.is_some() {
            s.base_url = base_url;
        }
    }

    fn read(&self) -> (Option<String>, Option<String>) {
        let s = self.session.read().unwrap();
        (s.token.clone(), s.base_url.clone())
    }
}

impl Default for AssetProxy {
    fn default() -> Self {
        Self::new()
    }
}

/// JS sets the bearer + API base so the handler can authenticate proxied fetches.
/// Called from the transport on `setSession`, token refresh, and logout (null).
#[tauri::command]
pub fn set_asset_proxy_session(
    proxy: tauri::State<'_, AssetProxy>,
    token: Option<String>,
    base_url: Option<String>,
) {
    proxy.set(token, base_url);
}

/// A response with no body, for a load the proxy answers itself.
fn status_only(status: StatusCode) -> Response<Cow<'static, [u8]>> {
    let mut resp = Response::new(Cow::Owned(Vec::new()));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    resp
}

/// The request the proxy makes for one webview load.
#[derive(Debug, PartialEq)]
struct Upstream {
    url: String,
    headers: Vec<(header::HeaderName, String)>,
}

/// The upstream GET for `path`: the API base plus the path, with the bearer
/// when signed in and the webview's `Range` (see [`clamp_range`]) when it sent
/// one. `None` only when no server is configured yet. Signed out, the request
/// still goes, without a bearer: public files (the workspace's logo on the
/// sign-in and invitation pages) load, and the server answers 401 for anything
/// else.
fn upstream(
    token: Option<&str>,
    base_url: Option<&str>,
    path: &str,
    range: Option<&str>,
) -> Option<Upstream> {
    let base_url = base_url?;
    let mut headers = Vec::new();
    if let Some(token) = token {
        headers.push((header::AUTHORIZATION, format!("Bearer {token}")));
    }
    if let Some(range) = range {
        headers.push((header::RANGE, clamp_range(range).into_owned()));
    }
    Some(Upstream {
        url: format!("{}{}", base_url.trim_end_matches('/'), path),
        headers,
    })
}

/// The `Range` to send upstream for the webview's `range`.
///
/// A single byte range from a start (`bytes=N-`, or `bytes=N-M` wider than
/// [`RANGE_CHUNK_BYTES`]) becomes the one chunk from that start. Everything
/// else goes as it came: a bounded range within the chunk, a suffix
/// (`bytes=-N`, which names its own size), and whatever the server's
/// `requested_range` doesn't serve as a range (multiple ranges, an inverted or
/// malformed range, another unit). The server answers those with the whole
/// file, as it would without the proxy. The grammar mirrors that function, so
/// the two agree on what a range is.
fn clamp_range(range: &str) -> Cow<'_, str> {
    let Some((start, end)) = from_range(range) else {
        return Cow::Borrowed(range);
    };
    let chunk_end = start.saturating_add(RANGE_CHUNK_BYTES - 1);
    match end {
        Some(end) if end <= chunk_end => Cow::Borrowed(range),
        _ => Cow::Owned(format!("bytes={start}-{chunk_end}")),
    }
}

/// `bytes=N-` or `bytes=N-M` (M at least N) as `(N, M)`, else `None`.
fn from_range(range: &str) -> Option<(u64, Option<u64>)> {
    let (unit, spec) = range.split_once('=')?;
    if !unit.trim().eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return None;
    }
    let (first, last) = spec.trim().split_once('-')?;
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let start = number(first)?;
    let end = if last.is_empty() {
        None
    } else {
        Some(number(last).filter(|&end| end >= start)?)
    };
    Some((start, end))
}

/// Make the upstream request and build the webview's response from it: the
/// server's status (200, 206, 416 or an error) with the headers a media element
/// and the cache need, and the body. The body for a range from a start is at
/// most one chunk; a suffix range is as large as it asks for.
async fn fetch(client: &reqwest::Client, upstream: Upstream) -> Response<Cow<'static, [u8]>> {
    let mut req = client.get(&upstream.url);
    for (name, value) in upstream.headers {
        req = req.header(name, value);
    }

    let upstream = match req.send().await {
        Ok(resp) => resp,
        Err(_) => return status_only(StatusCode::BAD_GATEWAY),
    };
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let body = match upstream.bytes().await {
        Ok(b) => Vec::from(b),
        Err(_) => return status_only(StatusCode::BAD_GATEWAY),
    };

    let mut builder = Response::builder().status(status.as_u16());
    // Carry through the headers the media element + cache need.
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::CONTENT_LENGTH,
        header::CACHE_CONTROL,
    ] {
        if let Some(value) = headers.get(&name) {
            builder = builder.header(name, value);
        }
    }
    // The webview `fetch()`es audio (the player reads the bytes for its
    // waveform) from the tauri://localhost origin, so the proxied response
    // needs CORS. Plain `<img>`/`<audio>` resource loads don't, but fetch
    // enforces it, which is why audio failed while images worked.
    builder = builder
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Accept-Ranges, Content-Length",
        );

    builder
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| status_only(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Proxy handler for the `nosdesk-asset` scheme.
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let proxy = ctx.app_handle().state::<AssetProxy>().inner().clone();
    let (token, base_url) = proxy.read();

    // The scheme path carries the full API path (e.g. `/api/files/tickets/1/x`).
    let path = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_default();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let Some(upstream) = upstream(
        token.as_deref(),
        base_url.as_deref(),
        &path,
        range.as_deref(),
    ) else {
        // No server configured yet.
        return responder.respond(status_only(StatusCode::UNAUTHORIZED));
    };

    let client = proxy.client.clone();
    tauri::async_runtime::spawn(async move {
        responder.respond(fetch(&client, upstream).await);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_out_a_load_goes_without_a_bearer() {
        let up = upstream(
            None,
            Some("https://help.example/"),
            "/uploads/branding/logo.png",
            None,
        )
        .expect("forwarded");
        assert_eq!(up.url, "https://help.example/uploads/branding/logo.png");
        assert!(up.headers.is_empty());
    }

    #[test]
    fn signed_in_a_load_carries_the_bearer_and_range() {
        let up = upstream(
            Some("tok"),
            Some("https://help.example"),
            "/api/files/tickets/1/a.mp4",
            Some("bytes=0-1023"),
        )
        .expect("forwarded");
        assert_eq!(up.url, "https://help.example/api/files/tickets/1/a.mp4");
        assert_eq!(
            up.headers,
            vec![
                (header::AUTHORIZATION, "Bearer tok".to_string()),
                (header::RANGE, "bytes=0-1023".to_string()),
            ]
        );
    }

    #[test]
    fn without_a_server_nothing_is_forwarded() {
        assert_eq!(upstream(Some("tok"), None, "/api/files/x", None), None);
    }

    // ── Range clamp ──────────────────────────────────────────────

    const CHUNK: u64 = RANGE_CHUNK_BYTES;

    #[test]
    fn an_open_ended_range_becomes_one_chunk() {
        assert_eq!(clamp_range("bytes=0-"), "bytes=0-4194303");
        assert_eq!(clamp_range("bytes=100-"), "bytes=100-4194403");
        assert_eq!(clamp_range(" bytes = 100- "), "bytes=100-4194403");
        // The unit is matched as the server matches it and sent lowercase.
        assert_eq!(clamp_range("Bytes=100-"), "bytes=100-4194403");
        // A start near the top of u64 saturates rather than wrapping.
        assert_eq!(
            clamp_range("bytes=18446744073709551615-"),
            "bytes=18446744073709551615-18446744073709551615"
        );
    }

    #[test]
    fn a_bounded_range_wider_than_a_chunk_becomes_one_chunk() {
        // iOS asks for the rest of the file as a bounded range.
        assert_eq!(clamp_range("bytes=0-104857599"), "bytes=0-4194303");
        assert_eq!(
            clamp_range("bytes=50000000-104857599"),
            "bytes=50000000-54194303"
        );
        assert_eq!(
            clamp_range(&format!("bytes=0-{CHUNK}")),
            format!("bytes=0-{}", CHUNK - 1)
        );
        assert_eq!(
            clamp_range("bytes=0-18446744073709551615"),
            "bytes=0-4194303"
        );
    }

    #[test]
    fn a_range_within_a_chunk_goes_as_it_came() {
        for range in [
            // iOS's probe.
            "bytes=0-1",
            "bytes=5-5",
            "bytes=0-1023",
            "Bytes=0-1023",
            &format!("bytes=0-{}", CHUNK - 1),
            &format!("bytes=1000-{}", 1000 + CHUNK - 1),
        ] {
            assert!(
                matches!(clamp_range(range), Cow::Borrowed(r) if r == range),
                "{range}"
            );
        }
    }

    #[test]
    fn a_suffix_range_goes_as_it_came() {
        // A suffix names its own size, and its start depends on a length the
        // proxy doesn't know.
        for range in ["bytes=-500", "bytes=-0", "bytes=-104857600"] {
            assert!(
                matches!(clamp_range(range), Cow::Borrowed(r) if r == range),
                "{range}"
            );
        }
    }

    #[test]
    fn what_the_server_does_not_serve_as_a_range_goes_as_it_came() {
        // The server's `requested_range_ignores_what_it_cannot_serve` list: it
        // answers each of these with the whole file.
        for range in [
            "",
            "bytes=",
            "bytes=-",
            "bytes=abc",
            "bytes=1-abc",
            "bytes=+1-2",
            "bytes=0x1-2",
            "bytes=5-2",
            "bytes=0-1,3-4",
            "bytes=0-,5-",
            "bytes=0-1, -5",
            "items=0-1",
            "items=0-",
            "0-1",
            "bytes=99999999999999999999999-",
        ] {
            assert!(
                matches!(clamp_range(range), Cow::Borrowed(r) if r == range),
                "{range:?}"
            );
        }
    }

    #[test]
    fn the_clamped_range_is_what_goes_upstream() {
        let up = upstream(
            Some("tok"),
            Some("https://help.example"),
            "/api/files/tickets/1/a.mov",
            Some("bytes=0-"),
        )
        .expect("forwarded");
        assert_eq!(
            up.headers,
            vec![
                (header::AUTHORIZATION, "Bearer tok".to_string()),
                (header::RANGE, "bytes=0-4194303".to_string()),
            ]
        );
    }

    // ── Against a local file server ──────────────────────────────

    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex;

    /// The `Range` and `Authorization` of each request a [`FileServer`] took.
    type Seen = Mutex<Vec<(Option<String>, Option<String>)>>;

    /// Larger than two chunks, and not a multiple of one.
    const TOTAL: u64 = 2 * RANGE_CHUNK_BYTES + 1_000;

    fn byte_at(i: u64) -> u8 {
        (i % 251) as u8
    }

    /// A one-file server on a local port that answers ranges as the Nosdesk
    /// server does: 206 for one range, 416 past the end, the whole file for no
    /// range or one it doesn't serve. It records the `Range` and
    /// `Authorization` of each request.
    struct FileServer {
        base: String,
        seen: Arc<Seen>,
    }

    impl FileServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let base = format!("http://{}", listener.local_addr().unwrap());
            let seen = Arc::new(Mutex::new(Vec::new()));
            let log = seen.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    serve(stream, &log);
                }
            });
            Self { base, seen }
        }

        /// What the proxy sends for a webview load with `range`.
        fn get(&self, range: Option<&str>) -> Response<Cow<'static, [u8]>> {
            let up = upstream(
                Some("tok"),
                Some(&self.base),
                "/api/files/tickets/1/clip.mov",
                range,
            )
            .expect("forwarded");
            tauri::async_runtime::block_on(fetch(&reqwest::Client::new(), up))
        }

        /// The `Range` the server received on the last request.
        fn last_range(&self) -> Option<String> {
            let seen = self.seen.lock().unwrap();
            let (range, auth) = seen.last().cloned().expect("a request");
            assert_eq!(auth.as_deref(), Some("Bearer tok"));
            range
        }
    }

    fn serve(stream: TcpStream, log: &Seen) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let (mut range, mut auth) = (None, None);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.trim_end().split_once(':') {
                let value = Some(value.trim().to_string());
                if name.eq_ignore_ascii_case("range") {
                    range = value;
                } else if name.eq_ignore_ascii_case("authorization") {
                    auth = value;
                }
            }
        }
        let answer = answer(range.as_deref());
        log.lock().unwrap().push((range, auth));

        let mut head = String::new();
        let body = match answer {
            Answer::Whole => {
                head.push_str("HTTP/1.1 200 OK\r\n");
                0..TOTAL
            }
            Answer::Part(start, end) => {
                head.push_str("HTTP/1.1 206 Partial Content\r\n");
                head.push_str(&format!("Content-Range: bytes {start}-{end}/{TOTAL}\r\n"));
                start..end + 1
            }
            Answer::Unsatisfiable => {
                head.push_str("HTTP/1.1 416 Range Not Satisfiable\r\n");
                head.push_str(&format!("Content-Range: bytes */{TOTAL}\r\n"));
                0..0
            }
        };
        if !matches!(answer, Answer::Unsatisfiable) {
            head.push_str("Content-Type: video/quicktime\r\n");
            head.push_str("Cache-Control: private, max-age=3600\r\n");
        }
        head.push_str("Accept-Ranges: bytes\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.end - body.start));
        head.push_str("Connection: close\r\n\r\n");
        let body: Vec<u8> = body.map(byte_at).collect();
        let mut stream = stream;
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
    }

    enum Answer {
        Whole,
        Part(u64, u64),
        Unsatisfiable,
    }

    fn answer(range: Option<&str>) -> Answer {
        let Some(range) = range else {
            return Answer::Whole;
        };
        if let Some(n) = range.strip_prefix("bytes=-") {
            return match n.parse::<u64>() {
                Ok(0) => Answer::Unsatisfiable,
                Ok(n) => Answer::Part(TOTAL.saturating_sub(n), TOTAL - 1),
                Err(_) => Answer::Whole,
            };
        }
        match from_range(range) {
            Some((start, _)) if start >= TOTAL => Answer::Unsatisfiable,
            Some((start, end)) => Answer::Part(start, end.map_or(TOTAL - 1, |e| e.min(TOTAL - 1))),
            None => Answer::Whole,
        }
    }

    fn header_of<'a>(
        resp: &'a Response<Cow<'static, [u8]>>,
        name: header::HeaderName,
    ) -> Option<&'a str> {
        resp.headers().get(name).map(|v| v.to_str().unwrap())
    }

    fn assert_part(resp: &Response<Cow<'static, [u8]>>, start: u64, end: u64) {
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            header_of(resp, header::CONTENT_RANGE),
            Some(format!("bytes {start}-{end}/{TOTAL}").as_str())
        );
        let len = end - start + 1;
        assert_eq!(
            header_of(resp, header::CONTENT_LENGTH),
            Some(len.to_string().as_str())
        );
        assert_eq!(
            header_of(resp, header::CONTENT_TYPE),
            Some("video/quicktime")
        );
        assert_eq!(header_of(resp, header::ACCEPT_RANGES), Some("bytes"));
        assert_eq!(
            header_of(resp, header::CACHE_CONTROL),
            Some("private, max-age=3600")
        );
        assert_eq!(
            header_of(resp, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some("*")
        );
        assert_eq!(resp.body().len() as u64, len);
        assert!(resp
            .body()
            .iter()
            .zip(start..)
            .all(|(&b, i)| b == byte_at(i)));
    }

    #[test]
    fn an_open_ended_load_gets_one_chunk_with_the_servers_headers() {
        let server = FileServer::start();
        let resp = server.get(Some("bytes=100-"));
        assert_eq!(server.last_range().as_deref(), Some("bytes=100-4194403"));
        assert_part(&resp, 100, 100 + CHUNK - 1);
    }

    #[test]
    fn a_load_to_the_end_of_the_file_gets_one_chunk() {
        let server = FileServer::start();
        let resp = server.get(Some(&format!("bytes=0-{}", TOTAL - 1)));
        assert_eq!(server.last_range().as_deref(), Some("bytes=0-4194303"));
        assert_part(&resp, 0, CHUNK - 1);
    }

    #[test]
    fn the_last_chunk_is_as_short_as_the_file() {
        let server = FileServer::start();
        let start = TOTAL - 1_000;
        let resp = server.get(Some(&format!("bytes={start}-")));
        assert_eq!(
            server.last_range(),
            Some(format!("bytes={start}-{}", start + CHUNK - 1))
        );
        assert_part(&resp, start, TOTAL - 1);
    }

    #[test]
    fn the_probe_and_a_suffix_pass_through() {
        let server = FileServer::start();
        let resp = server.get(Some("bytes=0-1"));
        assert_eq!(server.last_range().as_deref(), Some("bytes=0-1"));
        assert_part(&resp, 0, 1);

        let resp = server.get(Some("bytes=-500"));
        assert_eq!(server.last_range().as_deref(), Some("bytes=-500"));
        assert_part(&resp, TOTAL - 500, TOTAL - 1);
    }

    #[test]
    fn a_start_past_the_end_passes_the_416_through() {
        let server = FileServer::start();
        let resp = server.get(Some(&format!("bytes={TOTAL}-")));
        assert_eq!(
            server.last_range(),
            Some(format!("bytes={TOTAL}-{}", TOTAL + CHUNK - 1))
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            header_of(&resp, header::CONTENT_RANGE),
            Some(format!("bytes */{TOTAL}").as_str())
        );
        assert_eq!(header_of(&resp, header::ACCEPT_RANGES), Some("bytes"));
        assert!(resp.body().is_empty());
    }

    #[test]
    fn a_load_without_a_range_gets_the_whole_file_as_before() {
        let server = FileServer::start();
        let resp = server.get(None);
        assert_eq!(server.last_range(), None);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(header_of(&resp, header::CONTENT_RANGE), None);
        assert_eq!(
            header_of(&resp, header::CONTENT_LENGTH),
            Some(TOTAL.to_string().as_str())
        );
        assert_eq!(header_of(&resp, header::ACCEPT_RANGES), Some("bytes"));
        assert_eq!(resp.body().len() as u64, TOTAL);
    }

    #[test]
    fn a_multi_range_goes_as_it_came_and_the_server_sends_the_whole_file() {
        let server = FileServer::start();
        let resp = server.get(Some("bytes=0-1,5-6"));
        assert_eq!(server.last_range().as_deref(), Some("bytes=0-1,5-6"));
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.body().len() as u64, TOTAL);
    }

    #[test]
    fn an_unreachable_server_is_a_bad_gateway() {
        let base = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            format!("http://{}", listener.local_addr().unwrap())
        };
        let up = upstream(Some("tok"), Some(&base), "/api/files/x", Some("bytes=0-"))
            .expect("forwarded");
        let resp = tauri::async_runtime::block_on(fetch(&reqwest::Client::new(), up));
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            header_of(&resp, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some("*")
        );
    }
}
