//! Bounded local backing for non-seekable Docker archive content.

use std::fs::File;
use std::os::unix::fs::{FileExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use tokio::io::AsyncWriteExt;
use tuxstack_vfs::VfsError;

pub type ContentByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, VfsError>> + Send + 'static>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSpoolLimits {
    pub memory_threshold: u64,
    pub max_single_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for ContentSpoolLimits {
    fn default() -> Self {
        Self {
            memory_threshold: 2 * 1024 * 1024,
            max_single_bytes: 2 * 1024 * 1024 * 1024,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl ContentSpoolLimits {
    pub fn validate(&self) -> Result<(), VfsError> {
        if self.memory_threshold > self.max_single_bytes
            || self.max_single_bytes == 0
            || self.max_single_bytes > self.max_total_bytes
        {
            return Err(VfsError::InvalidInput("invalid content spool limits"));
        }
        Ok(())
    }
}

struct SpoolInner {
    runtime_directory: PathBuf,
    limits: ContentSpoolLimits,
    reserved_bytes: AtomicU64,
}

#[derive(Clone)]
pub struct ContentSpool {
    inner: Arc<SpoolInner>,
}

impl ContentSpool {
    pub async fn new(
        runtime_directory: impl Into<PathBuf>,
        limits: ContentSpoolLimits,
    ) -> Result<Self, VfsError> {
        limits.validate()?;
        let runtime_directory = runtime_directory.into();
        tokio::fs::create_dir_all(&runtime_directory).await?;
        tokio::fs::set_permissions(&runtime_directory, std::fs::Permissions::from_mode(0o700))
            .await?;
        Ok(Self {
            inner: Arc::new(SpoolInner {
                runtime_directory,
                limits,
                reserved_bytes: AtomicU64::new(0),
            }),
        })
    }

    pub fn limits(&self) -> &ContentSpoolLimits {
        &self.inner.limits
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.inner.reserved_bytes.load(Ordering::Acquire)
    }

    pub async fn spool(&self, mut stream: ContentByteStream) -> Result<ContentBacking, VfsError> {
        let mut writer = self.writer();
        while let Some(chunk) = stream.next().await {
            writer.push(chunk?).await?;
        }
        writer.finish().await
    }

    /// Incremental sink used by tar extraction. It preserves the same memory,
    /// single-file, and aggregate accounting as `spool` without buffering an
    /// archive entry before deciding its backing strategy.
    pub fn writer(&self) -> ContentSpoolWriter {
        ContentSpoolWriter {
            spool: self.clone(),
            memory: BytesMut::new(),
            temporary: None,
            reserved: 0,
            finished: false,
        }
    }

    pub fn memory(&self, bytes: Bytes) -> Result<ContentBacking, VfsError> {
        if bytes.len() as u64 > self.inner.limits.max_single_bytes {
            return Err(VfsError::Unavailable(
                "single content spool limit exceeded".into(),
            ));
        }
        self.reserve(bytes.len() as u64)?;
        let lease = Arc::new(ContentLease {
            owner: Arc::clone(&self.inner),
            bytes: bytes.len() as u64,
            path: None,
        });
        Ok(ContentBacking::Memory {
            bytes: Arc::new(bytes),
            lease,
        })
    }

    fn reserve(&self, bytes: u64) -> Result<(), VfsError> {
        self.inner
            .reserved_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|next| *next <= self.inner.limits.max_total_bytes)
            })
            .map(|_| ())
            .map_err(|_| VfsError::Unavailable("total content spool limit exceeded".into()))
    }

    fn release(&self, bytes: u64) {
        self.inner.reserved_bytes.fetch_sub(bytes, Ordering::AcqRel);
    }

    async fn create_temporary(&self) -> Result<(PathBuf, tokio::fs::File), VfsError> {
        let path = self
            .inner
            .runtime_directory
            .join(format!("content-{}.spool", uuid::Uuid::new_v4()));
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .await?;
        Ok((path, file))
    }
}

pub struct ContentSpoolWriter {
    spool: ContentSpool,
    memory: BytesMut,
    temporary: Option<(PathBuf, tokio::fs::File)>,
    reserved: u64,
    finished: bool,
}

impl ContentSpoolWriter {
    pub async fn push(&mut self, chunk: Bytes) -> Result<(), VfsError> {
        let next = self
            .reserved
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| VfsError::Unavailable("content spool size overflow".into()))?;
        if next > self.spool.inner.limits.max_single_bytes {
            return Err(VfsError::Unavailable(
                "single content spool limit exceeded".into(),
            ));
        }
        self.spool.reserve(chunk.len() as u64)?;
        self.reserved = next;

        if self.temporary.is_none() && next <= self.spool.inner.limits.memory_threshold {
            self.memory.extend_from_slice(&chunk);
            return Ok(());
        }
        if self.temporary.is_none() {
            let (path, mut file) = self.spool.create_temporary().await?;
            if let Err(error) = file.write_all(&self.memory).await {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(error.into());
            }
            self.memory.clear();
            self.temporary = Some((path, file));
        }
        if let Some((_, file)) = &mut self.temporary {
            file.write_all(&chunk).await?;
        }
        Ok(())
    }

    pub async fn finish(mut self) -> Result<ContentBacking, VfsError> {
        let lease = Arc::new(ContentLease {
            owner: Arc::clone(&self.spool.inner),
            bytes: self.reserved,
            path: self.temporary.as_ref().map(|(path, _)| path.clone()),
        });
        self.finished = true;
        self.reserved = 0;
        match self.temporary.take() {
            None => Ok(ContentBacking::Memory {
                bytes: Arc::new(self.memory.split().freeze()),
                lease,
            }),
            Some((path, mut file)) => {
                file.flush().await?;
                file.sync_data().await?;
                drop(file);
                let file = tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || File::open(path)
                })
                .await
                .map_err(|error| VfsError::Io(error.to_string()))??;
                Ok(ContentBacking::SpoolFile {
                    path,
                    file: Arc::new(file),
                    completed_bytes: lease.bytes,
                    lease,
                })
            }
        }
    }
}

impl Drop for ContentSpoolWriter {
    fn drop(&mut self) {
        if !self.finished {
            self.spool.release(self.reserved);
            if let Some((path, _)) = self.temporary.take() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[doc(hidden)]
pub struct ContentLease {
    owner: Arc<SpoolInner>,
    bytes: u64,
    path: Option<PathBuf>,
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        self.owner
            .reserved_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Clone)]
pub enum ContentBacking {
    DirectLocalFile {
        file: Arc<File>,
        length: u64,
    },
    Memory {
        bytes: Arc<Bytes>,
        lease: Arc<ContentLease>,
    },
    SpoolFile {
        path: PathBuf,
        file: Arc<File>,
        completed_bytes: u64,
        lease: Arc<ContentLease>,
    },
}

impl ContentBacking {
    pub fn direct_local_file(file: File) -> Result<Self, VfsError> {
        let length = file.metadata()?.len();
        Ok(Self::DirectLocalFile {
            file: Arc::new(file),
            length,
        })
    }

    pub fn len(&self) -> u64 {
        match self {
            Self::DirectLocalFile { length, .. } => *length,
            Self::Memory { bytes, .. } => bytes.len() as u64,
            Self::SpoolFile {
                completed_bytes, ..
            } => *completed_bytes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn spool_path(&self) -> Option<&Path> {
        match self {
            Self::SpoolFile { path, .. } => Some(path),
            _ => None,
        }
    }

    pub async fn read_at(&self, offset: u64, size: u32) -> Result<Bytes, VfsError> {
        match self {
            Self::Memory { bytes, .. } => {
                let start = usize::try_from(offset)
                    .unwrap_or(bytes.len())
                    .min(bytes.len());
                let end = start.saturating_add(size as usize).min(bytes.len());
                Ok(bytes.slice(start..end))
            }
            Self::DirectLocalFile { file, length } => {
                read_file_at(Arc::clone(file), *length, offset, size).await
            }
            Self::SpoolFile {
                file,
                completed_bytes,
                ..
            } => read_file_at(Arc::clone(file), *completed_bytes, offset, size).await,
        }
    }
}

async fn read_file_at(
    file: Arc<File>,
    length: u64,
    offset: u64,
    size: u32,
) -> Result<Bytes, VfsError> {
    if offset >= length || size == 0 {
        return Ok(Bytes::new());
    }
    let wanted = u64::from(size).min(length - offset) as usize;
    tokio::task::spawn_blocking(move || {
        let mut output = vec![0; wanted];
        let mut filled = 0;
        while filled < wanted {
            let read = file.read_at(&mut output[filled..], offset + filled as u64)?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        output.truncate(filled);
        Ok::<_, std::io::Error>(Bytes::from(output))
    })
    .await
    .map_err(|error| VfsError::Io(error.to_string()))?
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    #[tokio::test]
    async fn keeps_small_content_in_memory_and_supports_random_reads() {
        let directory = tempfile::tempdir().unwrap();
        let spool = ContentSpool::new(
            directory.path(),
            ContentSpoolLimits {
                memory_threshold: 16,
                max_single_bytes: 64,
                max_total_bytes: 64,
            },
        )
        .await
        .unwrap();
        let backing = spool
            .spool(Box::pin(stream::iter([Ok(Bytes::from_static(
                b"abcdefgh",
            ))])))
            .await
            .unwrap();
        assert!(matches!(backing, ContentBacking::Memory { .. }));
        assert_eq!(backing.read_at(3, 3).await.unwrap(), "def");
        assert_eq!(spool.reserved_bytes(), 8);
        drop(backing);
        assert_eq!(spool.reserved_bytes(), 0);
    }

    #[tokio::test]
    async fn spills_large_content_and_removes_file_on_last_drop() {
        let directory = tempfile::tempdir().unwrap();
        let spool = ContentSpool::new(
            directory.path(),
            ContentSpoolLimits {
                memory_threshold: 3,
                max_single_bytes: 64,
                max_total_bytes: 64,
            },
        )
        .await
        .unwrap();
        let backing = spool
            .spool(Box::pin(stream::iter([
                Ok(Bytes::from_static(b"abcd")),
                Ok(Bytes::from_static(b"efgh")),
            ])))
            .await
            .unwrap();
        let path = backing.spool_path().unwrap().to_path_buf();
        assert_eq!(backing.read_at(5, 10).await.unwrap(), "fgh");
        assert!(path.exists());
        drop(backing);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn enforces_single_and_aggregate_limits_without_leaking_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let spool = ContentSpool::new(
            directory.path(),
            ContentSpoolLimits {
                memory_threshold: 4,
                max_single_bytes: 8,
                max_total_bytes: 10,
            },
        )
        .await
        .unwrap();
        let first = spool.memory(Bytes::from_static(b"123456")).unwrap();
        let result = spool
            .spool(Box::pin(stream::iter([Ok(Bytes::from_static(b"abcde"))])))
            .await;
        assert!(matches!(result, Err(VfsError::Unavailable(_))));
        assert_eq!(spool.reserved_bytes(), 6);
        drop(first);
        assert_eq!(spool.reserved_bytes(), 0);
    }
}
