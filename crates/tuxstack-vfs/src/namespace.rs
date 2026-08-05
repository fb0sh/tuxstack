use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use bytes::Bytes;

use crate::{
    ConsistencyMode, ProviderCapabilities, ProviderDescriptor, ProviderFileHandle, ProviderKind,
    ReadOnlyFilesystemProvider, RequestContext, VfsError, VirtualDirectoryEntry, VirtualFileName,
    VirtualMetadata, VirtualPath, VirtualPathBytes,
};

#[derive(Clone)]
struct MountedProvider {
    key: String,
    provider: Arc<dyn ReadOnlyFilesystemProvider>,
}

#[derive(Clone)]
struct Alias {
    target: VirtualPathBytes,
}

struct OpenRoute {
    provider: Arc<dyn ReadOnlyFilesystemProvider>,
    inner: ProviderFileHandle,
}

pub struct ResolvedNamespaceProvider {
    pub key: String,
    pub provider: Arc<dyn ReadOnlyFilesystemProvider>,
    pub mount_path: VirtualPath,
    pub relative_path: VirtualPath,
}

/// Dynamic read-only namespace which routes operations to the provider mounted
/// at the deepest component-aware prefix. Synthetic parent directories and
/// friendly aliases are owned by the namespace, never guessed by providers.
pub struct NamespaceProvider {
    mounts: RwLock<HashMap<VirtualPath, MountedProvider>>,
    aliases: RwLock<HashMap<VirtualPath, Alias>>,
    handles: RwLock<HashMap<u64, OpenRoute>>,
    next_handle: AtomicU64,
}

impl NamespaceProvider {
    pub fn new() -> Self {
        Self {
            mounts: RwLock::new(HashMap::new()),
            aliases: RwLock::new(HashMap::new()),
            handles: RwLock::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
        }
    }

    pub fn mount(
        &self,
        path: VirtualPath,
        key: impl Into<String>,
        provider: Arc<dyn ReadOnlyFilesystemProvider>,
    ) -> Result<(), VfsError> {
        if path.is_root() {
            return Err(VfsError::InvalidInput("namespace root cannot be replaced"));
        }
        self.mounts
            .write()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?
            .insert(
                path,
                MountedProvider {
                    key: key.into(),
                    provider,
                },
            );
        Ok(())
    }

    pub fn alias(&self, path: VirtualPath, target: VirtualPathBytes) -> Result<(), VfsError> {
        if path.is_root() || !target.is_absolute() {
            return Err(VfsError::InvalidInput(
                "namespace alias must be absolute and non-root",
            ));
        }
        self.aliases
            .write()
            .map_err(|_| VfsError::Io("namespace alias lock poisoned".into()))?
            .insert(path, Alias { target });
        Ok(())
    }

    pub fn unmount(&self, path: &VirtualPath) -> Result<(), VfsError> {
        self.mounts
            .write()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?
            .remove(path);
        self.aliases
            .write()
            .map_err(|_| VfsError::Io("namespace alias lock poisoned".into()))?
            .retain(|alias, _| !alias.starts_with(path));
        Ok(())
    }

    pub fn clear(&self) -> Result<(), VfsError> {
        self.mounts
            .write()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?
            .clear();
        self.aliases
            .write()
            .map_err(|_| VfsError::Io("namespace alias lock poisoned".into()))?
            .clear();
        Ok(())
    }

    pub fn provider_at(
        &self,
        path: &VirtualPath,
    ) -> Result<Option<ResolvedNamespaceProvider>, VfsError> {
        let mounts = self
            .mounts
            .read()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?;
        Ok(mounts
            .iter()
            .filter(|(prefix, _)| path.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.depth())
            .and_then(|(prefix, mounted)| {
                path.strip_prefix(prefix)
                    .map(|relative_path| ResolvedNamespaceProvider {
                        key: mounted.key.clone(),
                        provider: Arc::clone(&mounted.provider),
                        mount_path: prefix.clone(),
                        relative_path,
                    })
            }))
    }

    fn synthetic_metadata(path: &VirtualPath) -> VirtualMetadata {
        let mut metadata = VirtualMetadata::directory(path.as_bytes());
        metadata.original.mode = 0o555;
        metadata
    }

    fn alias_metadata(path: &VirtualPath, target: &VirtualPathBytes) -> VirtualMetadata {
        VirtualMetadata::symlink(path.as_bytes(), target.as_bytes().len() as u64)
    }

    fn is_synthetic_directory(&self, path: &VirtualPath) -> Result<bool, VfsError> {
        if path.is_root() {
            return Ok(true);
        }
        let mounts = self
            .mounts
            .read()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?;
        Ok(mounts
            .keys()
            .any(|mount| mount.starts_with(path) && mount != path))
    }

    fn alias_for(&self, path: &VirtualPath) -> Result<Option<Alias>, VfsError> {
        Ok(self
            .aliases
            .read()
            .map_err(|_| VfsError::Io("namespace alias lock poisoned".into()))?
            .get(path)
            .cloned())
    }

    async fn metadata(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        if let Some(alias) = self.alias_for(path)? {
            return Ok(Self::alias_metadata(path, &alias.target));
        }
        if let Some(route) = self.provider_at(path)? {
            return route.provider.getattr(&route.relative_path, ctx).await;
        }
        if self.is_synthetic_directory(path)? {
            return Ok(Self::synthetic_metadata(path));
        }
        Err(VfsError::NotFound)
    }
}

impl Default for NamespaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReadOnlyFilesystemProvider for NamespaceProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            kind: ProviderKind::InMemory,
            consistency: ConsistencyMode::Live,
            source: Some("tuxstackd namespace".into()),
            capabilities: ProviderCapabilities::READ_ONLY,
        }
    }

    async fn lookup(
        &self,
        parent: &VirtualPath,
        name: &VirtualFileName,
        ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.metadata(&parent.join(name)?, ctx).await
    }

    async fn getattr(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<VirtualMetadata, VfsError> {
        self.metadata(path, ctx).await
    }

    async fn read_dir(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<Arc<Vec<VirtualDirectoryEntry>>, VfsError> {
        if let Some(route) = self.provider_at(path)? {
            return route.provider.read_dir(&route.relative_path, ctx).await;
        }
        if !self.is_synthetic_directory(path)? {
            return Err(VfsError::NotDirectory);
        }
        let mounts = self
            .mounts
            .read()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?
            .clone();
        let aliases = self
            .aliases
            .read()
            .map_err(|_| VfsError::Io("namespace alias lock poisoned".into()))?
            .clone();
        let mut children: BTreeMap<VirtualFileName, VirtualMetadata> = BTreeMap::new();
        for (mount, mounted) in mounts {
            if mount.parent().as_ref() == Some(path) {
                children.insert(
                    mount
                        .file_name()
                        .ok_or(VfsError::InvalidInput("mount has no name"))?
                        .clone(),
                    mounted.provider.getattr(&VirtualPath::root(), ctx).await?,
                );
            } else if mount.starts_with(path) && mount.depth() > path.depth() {
                let child = mount.components()[path.depth()].clone();
                let child_path = path.join(&child)?;
                children
                    .entry(child)
                    .or_insert_with(|| Self::synthetic_metadata(&child_path));
            }
        }
        for (alias_path, alias) in aliases {
            if alias_path.parent().as_ref() == Some(path) {
                children.insert(
                    alias_path
                        .file_name()
                        .ok_or(VfsError::InvalidInput("alias has no name"))?
                        .clone(),
                    Self::alias_metadata(&alias_path, &alias.target),
                );
            }
        }
        Ok(Arc::new(
            children
                .into_iter()
                .map(|(name, metadata)| VirtualDirectoryEntry { name, metadata })
                .collect(),
        ))
    }

    async fn read_link(
        &self,
        path: &VirtualPath,
        ctx: &RequestContext,
    ) -> Result<VirtualPathBytes, VfsError> {
        if let Some(alias) = self.alias_for(path)? {
            return Ok(alias.target);
        }
        let Some(route) = self.provider_at(path)? else {
            return Err(VfsError::InvalidInput("node is not a symlink"));
        };
        let target = route.provider.read_link(&route.relative_path, ctx).await?;
        if !target.is_absolute() {
            return Ok(target);
        }

        // Absolute Docker links are rooted in this provider mount, never in
        // the host or the global TuxStack namespace.
        let provider_target = VirtualPath::from_absolute(target.as_bytes())?;
        let anchored = VirtualPath::from_components(
            route
                .mount_path
                .components()
                .iter()
                .chain(provider_target.components())
                .cloned(),
        )?;
        VirtualPathBytes::new(anchored.as_bytes())
    }

    async fn open(
        &self,
        path: &VirtualPath,
        flags: i32,
        ctx: &RequestContext,
    ) -> Result<ProviderFileHandle, VfsError> {
        let Some(route) = self.provider_at(path)? else {
            return Err(if self.is_synthetic_directory(path)? {
                VfsError::IsDirectory
            } else {
                VfsError::NotFound
            });
        };
        let inner = route
            .provider
            .open(&route.relative_path, flags, ctx)
            .await?;
        let id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.handles
            .write()
            .map_err(|_| VfsError::Io("namespace handle lock poisoned".into()))?
            .insert(
                id,
                OpenRoute {
                    provider: route.provider,
                    inner: inner.clone(),
                },
            );
        Ok(ProviderFileHandle {
            id,
            path: path.clone(),
            content_generation: inner.content_generation,
        })
    }

    async fn read_at(
        &self,
        handle: &ProviderFileHandle,
        offset: u64,
        size: u32,
        ctx: &RequestContext,
    ) -> Result<Bytes, VfsError> {
        let (provider, inner) = {
            let handles = self
                .handles
                .read()
                .map_err(|_| VfsError::Io("namespace handle lock poisoned".into()))?;
            let route = handles.get(&handle.id).ok_or(VfsError::BadHandle)?;
            (Arc::clone(&route.provider), route.inner.clone())
        };
        provider.read_at(&inner, offset, size, ctx).await
    }

    async fn close(&self, handle: ProviderFileHandle) -> Result<(), VfsError> {
        let route = self
            .handles
            .write()
            .map_err(|_| VfsError::Io("namespace handle lock poisoned".into()))?
            .remove(&handle.id)
            .ok_or(VfsError::BadHandle)?;
        route.provider.close(route.inner).await
    }

    async fn refresh(&self, path: Option<&VirtualPath>) -> Result<(), VfsError> {
        if let Some(path) = path {
            if let Some(route) = self.provider_at(path)? {
                return route.provider.refresh(Some(&route.relative_path)).await;
            }
        }
        let providers: Vec<_> = self
            .mounts
            .read()
            .map_err(|_| VfsError::Io("namespace mount lock poisoned".into()))?
            .values()
            .map(|mounted| Arc::clone(&mounted.provider))
            .collect();
        for provider in providers {
            provider.refresh(None).await?;
        }
        Ok(())
    }
}
