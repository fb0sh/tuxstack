use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use bytes::Bytes;

use crate::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName, VirtualFileType,
    VirtualMetadata, VirtualPath, VirtualPathBytes,
};

#[async_trait]
pub trait ReadOnlyFilesystemProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError>;

    async fn getattr(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError>;

    async fn read_dir(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError>;

    async fn read_link(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError>;

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        ctx: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError>;

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        ctx: &RequestContext,
    ) -> Result<Bytes, VfsError>;

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError>;

    async fn refresh(&self, path: Option<&VirtualPath>) -> Result<(), VfsError>;
}

#[derive(Clone, Debug)]
enum MemoryContent {
    Directory,
    File(Bytes),
    Symlink(VirtualPathBytes),
    Special,
}

#[derive(Clone, Debug)]
struct MemoryNode {
    metadata: VirtualMetadata,
    content: MemoryContent,
}

#[derive(Debug)]
pub struct InMemoryProvider {
    descriptor: ProviderDescriptor,
    nodes: RwLock<HashMap<VirtualPath, MemoryNode>>,
    next_handle: AtomicU64,
}

impl InMemoryProvider {
    pub fn new() -> Self {
        let root = VirtualPath::root();
        let nodes = HashMap::from([(
            root,
            MemoryNode {
                metadata: VirtualMetadata::directory(b"root".to_vec()),
                content: MemoryContent::Directory,
            },
        )]);
        Self {
            descriptor: ProviderDescriptor {
                kind: ProviderKind::InMemory,
                consistency: ConsistencyMode::Immutable,
                source: None,
                capabilities: ProviderCapabilities::READ_ONLY,
            },
            nodes: RwLock::new(nodes),
            next_handle: AtomicU64::new(1),
        }
    }

    pub fn add_directory(
        &self,
        path: VirtualPath,
        node_id: impl Into<Vec<u8>>,
    ) -> Result<(), VfsError> {
        self.insert(
            path,
            MemoryNode {
                metadata: VirtualMetadata::directory(node_id),
                content: MemoryContent::Directory,
            },
        )
    }

    pub fn add_file(
        &self,
        path: VirtualPath,
        node_id: impl Into<Vec<u8>>,
        content: impl Into<Bytes>,
    ) -> Result<(), VfsError> {
        let content = content.into();
        self.insert(
            path,
            MemoryNode {
                metadata: VirtualMetadata::file(node_id, content.len() as u64),
                content: MemoryContent::File(content),
            },
        )
    }

    pub fn add_symlink(
        &self,
        path: VirtualPath,
        node_id: impl Into<Vec<u8>>,
        target: VirtualPathBytes,
    ) -> Result<(), VfsError> {
        let length = target.as_bytes().len() as u64;
        self.insert(
            path,
            MemoryNode {
                metadata: VirtualMetadata::symlink(node_id, length),
                content: MemoryContent::Symlink(target),
            },
        )
    }

    pub fn add_special(
        &self,
        path: VirtualPath,
        node_id: impl Into<Vec<u8>>,
        file_type: VirtualFileType,
        device_id: u64,
    ) -> Result<(), VfsError> {
        if !file_type.is_special() {
            return Err(VfsError::InvalidInput("node is not a special file"));
        }
        self.insert(
            path,
            MemoryNode {
                metadata: VirtualMetadata::special(node_id, file_type, device_id),
                content: MemoryContent::Special,
            },
        )
    }

    fn insert(&self, path: VirtualPath, node: MemoryNode) -> Result<(), VfsError> {
        if path.is_root() {
            return Err(VfsError::InvalidInput("root cannot be replaced"));
        }
        let parent = path
            .parent()
            .ok_or(VfsError::InvalidInput("node has no parent"))?;
        let mut nodes = self
            .nodes
            .write()
            .expect("in-memory provider lock poisoned");
        match nodes.get(&parent) {
            Some(parent) if parent.metadata.file_type == VirtualFileType::Directory => {}
            Some(_) => return Err(VfsError::NotDirectory),
            None => return Err(VfsError::NotFound),
        }
        nodes.insert(path, node);
        Ok(())
    }

    fn node(&self, path: &VirtualPath) -> Result<MemoryNode, VfsError> {
        self.nodes
            .read()
            .expect("in-memory provider lock poisoned")
            .get(path)
            .cloned()
            .ok_or(VfsError::NotFound)
    }
}

impl Default for InMemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for InMemoryProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.node(&parent.join(name)?).map(|node| node.metadata)
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.node(path).map(|node| node.metadata)
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        if self.node(path)?.metadata.file_type != VirtualFileType::Directory {
            return Err(VfsError::NotDirectory);
        }
        let nodes = self.nodes.read().expect("in-memory provider lock poisoned");
        let mut entries: Vec<_> = nodes
            .iter()
            .filter(|(candidate, _)| candidate.parent().as_ref() == Some(path))
            .map(|(candidate, node)| VirtualDirectoryEntry {
                name: candidate.file_name().expect("non-root child").clone(),
                metadata: node.metadata.clone(),
            })
            .collect();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Arc::new(entries))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        _ctx: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        match self.node(path)?.content {
            MemoryContent::Symlink(target) => Ok(target),
            _ => Err(VfsError::InvalidInput("node is not a symlink")),
        }
    }

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        _ctx: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        if !crate::is_read_only_open(flags) {
            return Err(VfsError::ReadOnly);
        }
        let node = self.node(path)?;
        match node.metadata.file_type {
            VirtualFileType::RegularFile => Ok(ProviderFileHandle {
                id: self.next_handle.fetch_add(1, Ordering::Relaxed),
                path: path.clone(),
                content_generation: node.metadata.generation,
            }),
            VirtualFileType::Directory => Err(VfsError::IsDirectory),
            VirtualFileType::Symlink => Err(VfsError::InvalidInput("cannot open a symlink")),
            _ => Err(VfsError::SpecialFile),
        }
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        _ctx: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        let node = self.node(&handle.path)?;
        if node.metadata.generation != handle.content_generation {
            return Err(VfsError::Stale);
        }
        let MemoryContent::File(content) = node.content else {
            return Err(VfsError::BadHandle);
        };
        let start = usize::try_from(offset)
            .unwrap_or(content.len())
            .min(content.len());
        let end = start.saturating_add(size as usize).min(content.len());
        Ok(content.slice(start..end))
    }

    async fn close(&self, _handle: ProviderFileHandle) -> Result<(), VfsError> {
        Ok(())
    }

    async fn refresh(&self, _path: Option<&VirtualPath>) -> Result<(), VfsError> {
        Ok(())
    }
}
