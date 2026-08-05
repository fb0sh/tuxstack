#![forbid(unsafe_code)]

mod adapter;
mod error;
mod executor;
mod handle;
mod inode;
mod model;
mod path;
mod provider;
mod router;
mod symlink;

pub use adapter::{InvalidationNotifier, ReadOnlyFuseAdapter, is_read_only_open};
pub use error::VfsError;
pub use executor::ProviderExecutor;
pub use handle::{OpenHandle, OpenHandleId, OpenHandleTable};
pub use inode::{HardlinkKey, InodeRecord, InodeTable, ROOT_INODE, VirtualNodeKey};
pub use model::{
    ConsistencyMode, DockerFilesystemResource, ImagePlatform, OriginalMetadata,
    ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind, RequestContext,
    VirtualDirectoryEntry, VirtualFileType, VirtualMetadata,
};
pub use path::{
    DEFAULT_MAX_PATH_BYTES, FuseNameCodec, MAX_NAME_BYTES, VirtualFileName, VirtualPath,
    VirtualPathBytes,
};
pub use provider::{InMemoryProvider, ReadOnlyFilesystemProvider};
pub use router::{
    ContainerPath, ContainerPathRouter, ProviderKey, ResolvedContainerMount, ResolvedRoute,
};
pub use symlink::{
    DEFAULT_MAX_SYMLINK_DEPTH, resolve_symlink_chain, resolve_target, rewrite_symlink,
};
