//! An upload streamed to a local temp file under a hard byte cap, and
//! copied to and from workspace storage without holding it in memory.
//!
//! The cap is the caller's and is checked before each chunk is written, so
//! a too-large upload stops at the first chunk past it. `MAX_FILE_SIZE_MB`
//! doesn't apply: it sizes the urlencoded form config, not a multipart
//! stream a handler reads itself.
//!
//! The temp file is removed when the [`Spooled`] drops, on success or
//! failure alike.

use std::io;
use std::path::Path;

use actix_multipart::Field;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

use crate::services::imports::refusal::Refused;
use crate::utils::storage::{Storage, StorageError, StoredFile};

/// An upload on local disk.
#[derive(Debug)]
pub struct Spooled {
    file: NamedTempFile,
    /// The name the client gave, if any. Display only: never a path.
    pub filename: Option<String>,
    pub size: u64,
}

impl Spooled {
    /// The temp file's path, for [`Storage::put_file_from_path`].
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// A new read handle at the start of the file, for
    /// [`super::SafeArchive::open`]. The file stays until `self` drops.
    pub fn open(&self) -> io::Result<std::fs::File> {
        self.file.reopen()
    }

    /// The temp file itself, for a caller that outlives this value.
    pub fn into_temp(self) -> NamedTempFile {
        self.file
    }
}

#[derive(Debug)]
pub enum SpoolError {
    /// Past the cap; nothing past it was written.
    TooLarge { max: u64 },
    /// The client's stream failed (disconnect, malformed multipart).
    Stream(String),
    /// The temp file couldn't be written.
    Io(io::Error),
}

impl std::fmt::Display for SpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { max } => write!(f, "The file is larger than {max} bytes."),
            Self::Stream(e) => write!(f, "The upload couldn't be read: {e}"),
            Self::Io(e) => write!(f, "The upload couldn't be saved: {e}"),
        }
    }
}

impl std::error::Error for SpoolError {}

impl From<io::Error> for SpoolError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<SpoolError> for Refused {
    fn from(e: SpoolError) -> Self {
        match e {
            SpoolError::TooLarge { max } => {
                Refused::new("upload_too_large", e.to_string()).arg("max", max)
            }
            SpoolError::Stream(_) => Refused::new("upload_unreadable", e.to_string()),
            SpoolError::Io(_) => Refused::new("upload_not_saved", e.to_string()),
        }
    }
}

fn temp_file(temp_dir: Option<&Path>) -> io::Result<NamedTempFile> {
    match temp_dir {
        Some(dir) => NamedTempFile::new_in(dir),
        None => NamedTempFile::new(),
    }
}

/// Write `stream` to a temp file, refusing it at the first chunk that takes
/// it past `max_bytes`.
pub async fn spool<S, E>(
    mut stream: S,
    max_bytes: u64,
    temp_dir: Option<&Path>,
) -> Result<Spooled, SpoolError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let file = temp_file(temp_dir)?;
    let mut out = tokio::fs::File::from_std(file.as_file().try_clone()?);
    let mut size: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| SpoolError::Stream(e.to_string()))?;
        size += chunk.len() as u64;
        if size > max_bytes {
            return Err(SpoolError::TooLarge { max: max_bytes });
        }
        out.write_all(&chunk).await?;
    }
    out.flush().await?;
    Ok(Spooled {
        file,
        filename: None,
        size,
    })
}

/// [`spool`] for a multipart field, keeping the name the client gave.
pub async fn spool_field(
    field: &mut Field,
    max_bytes: u64,
    temp_dir: Option<&Path>,
) -> Result<Spooled, SpoolError> {
    let filename = field
        .content_disposition()
        .get_filename()
        .map(str::to_string);
    let mut spooled = spool(field, max_bytes, temp_dir).await?;
    spooled.filename = filename;
    Ok(spooled)
}

/// Keep a spooled upload in storage under `key`, streamed from disk.
pub async fn store(
    storage: &dyn Storage,
    spooled: &Spooled,
    key: &str,
    content_type: &str,
) -> Result<StoredFile, StorageError> {
    storage
        .put_file_from_path(spooled.path(), key, content_type)
        .await
}

/// Copy a stored upload back to a temp file to read it (a zip needs seek).
pub async fn fetch(
    storage: &dyn Storage,
    key: &str,
    temp_dir: Option<&Path>,
) -> Result<Spooled, StorageError> {
    let file = temp_file(temp_dir)?;
    let size = storage.get_file_to_path(key, file.path()).await?;
    Ok(Spooled {
        file,
        filename: None,
        size,
    })
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::io::Read;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use futures::stream;

    use super::*;
    use crate::utils::storage::{LocalStorage, WorkspaceScopedStorage};

    fn chunks(parts: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
        stream::iter(
            parts
                .iter()
                .map(|p| Ok(Bytes::from_static(p)))
                .collect::<Vec<_>>(),
        )
    }

    #[tokio::test]
    async fn an_upload_within_the_cap_is_spooled_whole() {
        let spooled = spool(chunks(&[b"PK", b"zip", b"!"]), 6, None)
            .await
            .unwrap();
        assert_eq!(spooled.size, 6);
        let mut back = String::new();
        spooled.open().unwrap().read_to_string(&mut back).unwrap();
        assert_eq!(back, "PKzip!");
    }

    #[tokio::test]
    async fn the_cap_stops_a_too_large_upload_mid_stream() {
        let dir = tempfile::tempdir().unwrap();
        let pulled = Arc::new(AtomicUsize::new(0));
        let counter = pulled.clone();
        // An endless upload of 1 KiB chunks.
        let endless = stream::repeat_with(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, Infallible>(Bytes::from(vec![0u8; 1024]))
        });
        let err = spool(endless, 10 * 1024, Some(dir.path()))
            .await
            .unwrap_err();
        assert!(matches!(err, SpoolError::TooLarge { max } if max == 10 * 1024));
        assert_eq!(
            pulled.load(Ordering::SeqCst),
            11,
            "stops at the first chunk past the cap"
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the partial file is removed"
        );
        let refused: Refused = err.into();
        assert_eq!(refused.code, "upload_too_large");
    }

    #[tokio::test]
    async fn a_broken_stream_is_reported_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let broken = stream::iter(vec![Ok(Bytes::from_static(b"PK")), Err("reset by peer")]);
        let err = spool(broken, 1024, Some(dir.path())).await.unwrap_err();
        assert!(matches!(err, SpoolError::Stream(ref e) if e == "reset by peer"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_spooled_upload_round_trips_through_workspace_storage() {
        let root = tempfile::tempdir().unwrap();
        let base = Arc::new(LocalStorage::new(
            root.path().to_string_lossy().into_owned(),
            "/files".into(),
        ));
        let storage = WorkspaceScopedStorage::arc(base, 7);
        let spooled = spool(chunks(&[b"PK\x03\x04", b"rest"]), 1024, None)
            .await
            .unwrap();
        let stored = store(
            storage.as_ref(),
            &spooled,
            "workspace-imports/job/archive.zip",
            "application/zip",
        )
        .await
        .unwrap();
        assert_eq!(stored.path, "workspace-imports/job/archive.zip");
        assert_eq!(stored.size, 8);
        assert!(root
            .path()
            .join("ws/7/workspace-imports/job/archive.zip")
            .exists());

        let back = fetch(storage.as_ref(), "workspace-imports/job/archive.zip", None)
            .await
            .unwrap();
        assert_eq!(back.size, 8);
        assert_eq!(std::fs::read(back.path()).unwrap(), b"PK\x03\x04rest");

        assert!(matches!(
            fetch(storage.as_ref(), "workspace-imports/job/missing.zip", None).await,
            Err(StorageError::NotFound(_))
        ));
        assert!(matches!(
            store(
                storage.as_ref(),
                &spooled,
                "../escape.zip",
                "application/zip"
            )
            .await,
            Err(StorageError::InvalidPath(_))
        ));
    }

    #[tokio::test]
    async fn a_spooled_zip_opens_as_a_safe_archive() {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file("a.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"# A").unwrap();
        let bytes = Bytes::from(w.finish().unwrap().into_inner());
        let spooled = spool(
            stream::iter(vec![Ok::<_, Infallible>(bytes)]),
            1 << 20,
            None,
        )
        .await
        .unwrap();
        let mut archive = super::super::SafeArchive::open(
            spooled.open().unwrap(),
            &super::super::ArchiveOptions::documentation(),
        )
        .unwrap();
        assert_eq!(archive.read_text("a.md", 100).unwrap().text, "# A");
    }
}
