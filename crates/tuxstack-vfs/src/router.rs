use std::fmt;
use std::sync::Arc;

use crate::{ReadOnlyFilesystemProvider, VfsError, VirtualPath};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProviderKey(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ContainerPath(pub VirtualPath);

#[derive(Clone)]
pub struct ResolvedContainerMount {
    pub destination: VirtualPath,
    pub provider_key: ProviderKey,
    pub provider_root: VirtualPath,
    pub provider: Arc<dyn ReadOnlyFilesystemProvider>,
}

impl fmt::Debug for ResolvedContainerMount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedContainerMount")
            .field("destination", &self.destination)
            .field("provider_key", &self.provider_key)
            .field("provider_root", &self.provider_root)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct ResolvedRoute {
    pub container_path: ContainerPath,
    pub provider_key: ProviderKey,
    pub provider_path: VirtualPath,
    pub provider: Arc<dyn ReadOnlyFilesystemProvider>,
    pub mount: Option<ResolvedContainerMount>,
}

impl fmt::Debug for ResolvedRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedRoute")
            .field("container_path", &self.container_path)
            .field("provider_key", &self.provider_key)
            .field("provider_path", &self.provider_path)
            .field("mount", &self.mount)
            .finish_non_exhaustive()
    }
}

pub struct ContainerPathRouter {
    pub container_id: String,
    mounts: Vec<ResolvedContainerMount>,
    rootfs_key: ProviderKey,
    rootfs: Arc<dyn ReadOnlyFilesystemProvider>,
}

impl ContainerPathRouter {
    pub fn new(
        container_id: impl Into<String>,
        rootfs_key: ProviderKey,
        rootfs: Arc<dyn ReadOnlyFilesystemProvider>,
        mut mounts: Vec<ResolvedContainerMount>,
    ) -> Result<Self, VfsError> {
        if mounts.iter().any(|mount| mount.destination.is_root()) {
            return Err(VfsError::InvalidInput(
                "a mount cannot replace the container root",
            ));
        }
        mounts.sort_by(|left, right| {
            right
                .destination
                .depth()
                .cmp(&left.destination.depth())
                .then_with(|| {
                    left.destination
                        .as_bytes()
                        .cmp(&right.destination.as_bytes())
                })
        });
        Ok(Self {
            container_id: container_id.into(),
            mounts,
            rootfs_key,
            rootfs,
        })
    }

    pub fn mounts(&self) -> &[ResolvedContainerMount] {
        &self.mounts
    }

    /// Selects the deepest component-prefix mount and translates the remainder.
    /// Byte-string prefix matching is deliberately not used: `/app/data2` does not
    /// match a mount at `/app/data`.
    pub fn route(&self, path: &ContainerPath) -> Result<ResolvedRoute, VfsError> {
        if let Some(mount) = self
            .mounts
            .iter()
            .find(|mount| path.0.starts_with(&mount.destination))
        {
            let remainder = path
                .0
                .strip_prefix(&mount.destination)
                .expect("component prefix already checked");
            let provider_path = mount
                .provider_root
                .components()
                .iter()
                .chain(remainder.components())
                .cloned()
                .collect::<Vec<_>>();
            return Ok(ResolvedRoute {
                container_path: path.clone(),
                provider_key: mount.provider_key.clone(),
                provider_path: VirtualPath::from_components(provider_path)?,
                provider: Arc::clone(&mount.provider),
                mount: Some(mount.clone()),
            });
        }

        Ok(ResolvedRoute {
            container_path: path.clone(),
            provider_key: self.rootfs_key.clone(),
            provider_path: path.0.clone(),
            provider: Arc::clone(&self.rootfs),
            mount: None,
        })
    }
}
