use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tuxstack_vfs::{
    ConsistencyMode, InMemoryProvider, ProviderCapabilities, ProviderDescriptor, ProviderExecutor,
    ProviderFileHandle, ProviderKind, ReadOnlyFilesystemProvider, RequestContext, VfsError,
    VirtualDirectoryEntry, VirtualFileName, VirtualFileType, VirtualMetadata, VirtualPath,
    VirtualPathBytes, is_read_only_open,
};

fn path(value: &[u8]) -> VirtualPath {
    VirtualPath::from_absolute(value).unwrap()
}

fn context() -> RequestContext {
    RequestContext {
        uid: 1000,
        gid: 1000,
        pid: 1,
        request_id: 1,
    }
}

#[tokio::test]
async fn in_memory_provider_reads_files_and_directories() {
    let provider = InMemoryProvider::new();
    provider.add_directory(path(b"/dir"), b"dir").unwrap();
    provider
        .add_file(path(b"/dir/file"), b"file", Bytes::from_static(b"contents"))
        .unwrap();
    let entries = provider.read_dir(&path(b"/dir"), &context()).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name.as_bytes(), b"file");
    let handle = provider
        .open(&path(b"/dir/file"), libc::O_RDONLY, &context())
        .await
        .unwrap();
    assert_eq!(
        provider.read_at(&handle, 2, 4, &context()).await.unwrap(),
        Bytes::from_static(b"nten")
    );
    provider.close(handle).await.unwrap();
}

#[tokio::test]
async fn all_mutating_open_flags_are_read_only() {
    assert!(is_read_only_open(libc::O_RDONLY));
    assert!(is_read_only_open(libc::O_RDONLY | libc::O_CLOEXEC));
    for flags in [
        libc::O_WRONLY,
        libc::O_RDWR,
        libc::O_RDONLY | libc::O_CREAT,
        libc::O_RDONLY | libc::O_TRUNC,
        libc::O_RDONLY | libc::O_APPEND,
    ] {
        assert!(!is_read_only_open(flags));
    }

    let provider = InMemoryProvider::new();
    provider
        .add_file(path(b"/file"), b"file", Bytes::new())
        .unwrap();
    assert_eq!(
        provider
            .open(&path(b"/file"), libc::O_WRONLY, &context())
            .await,
        Err(VfsError::ReadOnly)
    );
}

#[tokio::test]
async fn special_devices_can_be_statted_but_never_opened() {
    let provider = InMemoryProvider::new();
    provider
        .add_special(
            path(b"/dev"),
            b"device",
            VirtualFileType::CharacterDevice,
            0x0103,
        )
        .unwrap();
    let metadata = provider.getattr(&path(b"/dev"), &context()).await.unwrap();
    assert_eq!(metadata.file_type, VirtualFileType::CharacterDevice);
    assert_eq!(metadata.device_id, Some(0x0103));
    assert_eq!(
        provider
            .open(&path(b"/dev"), libc::O_RDONLY, &context())
            .await,
        Err(VfsError::SpecialFile)
    );
    assert_eq!(VfsError::SpecialFile.errno(), libc::ENXIO);
}

#[test]
fn capabilities_have_no_write_bits_and_cover_required_contract() {
    let capabilities = ProviderCapabilities::READ_ONLY;
    for required in [
        ProviderCapabilities::LOOKUP,
        ProviderCapabilities::READDIR,
        ProviderCapabilities::GETATTR,
        ProviderCapabilities::READLINK,
        ProviderCapabilities::OPEN,
        ProviderCapabilities::READ,
        ProviderCapabilities::REFRESH,
    ] {
        assert!(capabilities.contains(required));
    }
    assert_eq!(capabilities.bits() & !0xff, 0);
}

struct SlowProvider;

#[async_trait]
impl ReadOnlyFilesystemProvider for SlowProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            kind: ProviderKind::InMemory,
            consistency: ConsistencyMode::Live,
            source: None,
            capabilities: ProviderCapabilities::READ_ONLY,
        }
    }
    async fn lookup(
        &self,
        _: &VirtualPath,
        _: &VirtualFileName,
        _: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        unreachable!()
    }
    async fn getattr(
        &self,
        _: &VirtualPath,
        _: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        unreachable!()
    }
    async fn read_dir(
        &self,
        _: &VirtualPath,
        _: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        unreachable!()
    }
    async fn read_link(
        &self,
        _: &VirtualPath,
        _: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        unreachable!()
    }
    async fn open(
        &self,
        _: &VirtualPath,
        _: i32,
        _: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        unreachable!()
    }
    async fn read_at(
        &self,
        _: &ProviderFileHandle,
        _: u64,
        _: u32,
        _: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        unreachable!()
    }
    async fn close(&self, _: ProviderFileHandle) -> Result<(), VfsError> {
        unreachable!()
    }
    async fn refresh(&self, _: Option<&VirtualPath>) -> Result<(), VfsError> {
        Ok(())
    }
}

#[test]
fn dedicated_executor_applies_per_operation_timeout_without_block_on() {
    let _provider = SlowProvider;
    let executor = ProviderExecutor::new(2, Duration::from_millis(20)).unwrap();
    let result: Result<(), VfsError> = executor.execute(async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    });
    assert_eq!(result, Err(VfsError::TimedOut));
    assert_eq!(ProviderExecutor::WORKER_THREADS, 4);
}
