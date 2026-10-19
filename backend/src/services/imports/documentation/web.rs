//! Images linked from the web, downloaded at the check through
//! `utils::safe_http` only: http and https, private addresses refused when
//! the name resolves (and as an address in the URL), 10 seconds and 10 MB
//! each, at most [`MAX_WEB_IMAGES`] per import, SVG refused. What the check
//! downloads is kept under `imports/{job}/web/` so Review shows what will
//! be imported, and the commit stores it with the page.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use uuid::Uuid;

use super::images::{self, MAX_IMAGE_BYTES};
use super::{web_key, ImageProblem, WebImage};
use crate::utils::safe_http::{self, SafeHttpError};
use crate::utils::storage::Storage;

/// Web images one import downloads.
pub const MAX_WEB_IMAGES: usize = 500;

/// Longest one download may take.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Downloads at once.
const CONCURRENCY: usize = 8;

/// Longest all of an import's downloads may take; what is left is reported
/// as not downloaded.
const ALL_DOWNLOADS: Duration = Duration::from_secs(5 * 60);

/// A downloaded image.
#[derive(Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Downloads images.
pub struct WebFetcher {
    client: reqwest::Client,
    /// Refuse a URL whose host is a private address before sending. Off
    /// only for the tests that serve images from a local server.
    check_address: bool,
}

impl WebFetcher {
    /// The fetcher every import uses: a `safe_http` client.
    pub fn safe() -> reqwest::Result<Self> {
        Ok(Self {
            client: safe_http::client(FETCH_TIMEOUT)?,
            check_address: true,
        })
    }

    /// A fetcher over any client, without the address check, for tests
    /// against a local server.
    #[cfg(test)]
    pub fn unchecked(client: reqwest::Client) -> Self {
        Self {
            client,
            check_address: false,
        }
    }

    /// Download one image, at most [`MAX_IMAGE_BYTES`], and check it is one
    /// the editor's upload would take.
    pub async fn fetch(&self, url: &str) -> Result<Fetched, ImageProblem> {
        if !images::is_web(url) {
            return Err(ImageProblem::RefusedAddress);
        }
        if self.check_address && safe_http::reject_unsafe_ip_literal(url).is_err() {
            return Err(ImageProblem::RefusedAddress);
        }
        let response = tokio::time::timeout(FETCH_TIMEOUT, self.client.get(url).send())
            .await
            .map_err(|_| ImageProblem::TimedOut)?
            .map_err(|e| problem_of(&e))?;
        if !response.status().is_success() {
            return Err(ImageProblem::FetchFailed);
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_IMAGE_BYTES)
        {
            return Err(ImageProblem::TooLarge);
        }
        let bytes = tokio::time::timeout(FETCH_TIMEOUT, read_capped(response))
            .await
            .map_err(|_| ImageProblem::TimedOut)??;
        let name = images::name_of(url, None);
        let content_type = images::validate(&bytes, &name)?;
        Ok(Fetched {
            bytes,
            content_type,
        })
    }
}

/// The body, refused at the first chunk past the cap.
async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, ImageProblem> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| problem_of(&e))? {
        if (bytes.len() + chunk.len()) as u64 > MAX_IMAGE_BYTES {
            return Err(ImageProblem::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// A failed request as the report words it: the resolver's refusal of a
/// private address, a timeout, or anything else.
fn problem_of(error: &reqwest::Error) -> ImageProblem {
    if error.is_timeout() {
        return ImageProblem::TimedOut;
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = source {
        if let Some(SafeHttpError::NonRoutable(_) | SafeHttpError::UnsupportedScheme) =
            e.downcast_ref::<SafeHttpError>()
        {
            return ImageProblem::RefusedAddress;
        }
        if e.to_string().contains("non-routable address") {
            return ImageProblem::RefusedAddress;
        }
        source = e.source();
    }
    ImageProblem::FetchFailed
}

/// Download every URL not already kept, keep each under the job's `web/`
/// prefix, and return what became of every URL. `kept` holds an earlier
/// check's results; an image it downloaded is not downloaded again.
pub async fn fetch_all(
    fetcher: &WebFetcher,
    storage: Arc<dyn Storage>,
    job_id: Uuid,
    urls: Vec<String>,
    kept: &BTreeMap<String, WebImage>,
) -> BTreeMap<String, WebImage> {
    let started = Instant::now();
    let mut out = BTreeMap::new();
    let mut to_fetch = Vec::new();
    for (i, url) in urls.into_iter().enumerate() {
        if let Some(previous) = kept.get(&url).filter(|w| w.key.is_some()) {
            out.insert(url, previous.clone());
            continue;
        }
        if i >= MAX_WEB_IMAGES {
            out.insert(url.clone(), failed(&url, ImageProblem::OverLimit));
            continue;
        }
        to_fetch.push(url);
    }
    let results: Vec<(String, WebImage)> = futures::stream::iter(to_fetch)
        .map(|url| {
            let storage = storage.clone();
            async move {
                if started.elapsed() > ALL_DOWNLOADS {
                    let result = failed(&url, ImageProblem::TimedOut);
                    return (url, result);
                }
                let result = match fetcher.fetch(&url).await {
                    Ok(fetched) => keep(storage.as_ref(), job_id, &url, fetched).await,
                    Err(problem) => failed(&url, problem),
                };
                (url, result)
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    out.extend(results);
    out
}

fn failed(url: &str, problem: ImageProblem) -> WebImage {
    WebImage {
        key: None,
        name: images::name_of(url, None),
        size: None,
        content_type: None,
        problem: Some(problem),
    }
}

async fn keep(storage: &dyn Storage, job_id: Uuid, url: &str, fetched: Fetched) -> WebImage {
    let extension = images::extension_for(&fetched.content_type).unwrap_or("bin");
    let key = web_key(job_id, url, extension);
    let size = fetched.bytes.len() as u64;
    match storage
        .put_file(&fetched.bytes, &key, &fetched.content_type)
        .await
    {
        Ok(_) => WebImage {
            key: Some(key),
            name: images::name_of(url, Some(&fetched.content_type)),
            size: Some(size),
            content_type: Some(fetched.content_type),
            problem: None,
        },
        Err(e) => {
            tracing::error!(id = %job_id, error = ?e, "failed to keep a web image");
            failed(url, ImageProblem::FetchFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server on a local port answering every request with `body` as
    /// `content_type`, for as many requests as `count`.
    fn serve(body: Vec<u8>, content_type: &'static str, count: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(count) {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}")
    }

    fn local() -> WebFetcher {
        WebFetcher::unchecked(reqwest::Client::builder().no_proxy().build().unwrap())
    }

    #[tokio::test]
    async fn an_image_downloads_and_is_typed_from_its_bytes() {
        let base = serve(super::super::images::tests::PNG.to_vec(), "text/plain", 1);
        let fetched = local().fetch(&format!("{base}/logo")).await.unwrap();
        assert_eq!(fetched.content_type, "image/png");
        assert_eq!(fetched.bytes, super::super::images::tests::PNG);
    }

    #[tokio::test]
    async fn an_image_over_ten_megabytes_is_refused() {
        // No Content-Length: the cap applies to what arrives.
        let base = serve(vec![0u8; (MAX_IMAGE_BYTES + 1) as usize], "image/png", 1);
        assert_eq!(
            local().fetch(&format!("{base}/big.png")).await.unwrap_err(),
            ImageProblem::TooLarge
        );
    }

    #[tokio::test]
    async fn an_svg_is_refused_whatever_it_is_called() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>".to_vec();
        let base = serve(svg, "image/png", 1);
        assert_eq!(
            local()
                .fetch(&format!("{base}/chart.png"))
                .await
                .unwrap_err(),
            ImageProblem::Svg
        );
    }

    #[tokio::test]
    async fn private_addresses_are_refused_by_the_safe_client() {
        let fetcher = WebFetcher::safe().unwrap();
        for url in [
            "http://127.0.0.1:9/a.png",
            "http://10.0.0.1/a.png",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/a.png",
            "ftp://example.com/a.png",
        ] {
            assert_eq!(
                fetcher.fetch(url).await.unwrap_err(),
                ImageProblem::RefusedAddress,
                "{url}"
            );
        }
        // A name that resolves to one is refused by the client's resolver
        // (`utils::safe_http`'s own tests); its message is read as the same
        // refusal.
    }

    #[tokio::test]
    async fn fetch_all_keeps_downloads_and_stops_at_the_limit() {
        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn Storage> = Arc::new(crate::utils::storage::LocalStorage::new(
            root.path().to_string_lossy().into_owned(),
            "/files".into(),
        ));
        let base = serve(super::super::images::tests::PNG.to_vec(), "image/png", 1);
        let job = Uuid::now_v7();
        let urls = vec![format!("{base}/a.png")];
        let out = fetch_all(
            &local(),
            storage.clone(),
            job,
            urls.clone(),
            &BTreeMap::new(),
        )
        .await;
        let first = &out[&urls[0]];
        assert_eq!(first.problem, None);
        assert_eq!(first.name, "a.png");
        let key = first.key.clone().unwrap();
        assert!(key.starts_with(&format!("imports/{job}/web/")), "{key}");
        assert_eq!(
            storage.get_file(&key).await.unwrap(),
            super::super::images::tests::PNG
        );

        // Past the limit, nothing more is asked for.
        let mut kept = BTreeMap::new();
        let mut many = Vec::new();
        for i in 0..MAX_WEB_IMAGES {
            let url = format!("https://example.com/{i}.png");
            kept.insert(url.clone(), first.clone());
            many.push(url);
        }
        many.push("https://example.com/one-more.png".to_string());
        let limited = fetch_all(&local(), storage.clone(), job, many.clone(), &kept).await;
        assert_eq!(
            limited[many.last().unwrap()].problem,
            Some(ImageProblem::OverLimit)
        );

        // A second check takes the kept download without asking again (the
        // server answers one request only).
        let again = fetch_all(&local(), storage, job, vec![urls[0].clone()], &out).await;
        assert_eq!(again[&urls[0]].key.as_deref(), Some(key.as_str()));
    }
}
