use std::sync::Arc;
use std::time::{Duration, Instant};

use slab::Slab;

use crate::{
    DockerFilesystemResource, ProviderFileHandle, ReadOnlyFilesystemProvider, VfsError, VirtualPath,
};

pub struct OpenHandle {
    pub provider: Arc<dyn ReadOnlyFilesystemProvider>,
    pub provider_handle: ProviderFileHandle,
    pub resource: DockerFilesystemResource,
    pub path: VirtualPath,
    pub content_generation: u64,
    pub backing_strategy: String,
    pub opened_at: Instant,
    pub last_accessed_at: Instant,
}

impl std::fmt::Debug for OpenHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenHandle")
            .field("provider_handle", &self.provider_handle)
            .field("resource", &self.resource)
            .field("path", &self.path)
            .field("content_generation", &self.content_generation)
            .field("backing_strategy", &self.backing_strategy)
            .field("opened_at", &self.opened_at)
            .field("last_accessed_at", &self.last_accessed_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OpenHandleId(pub u64);

#[derive(Debug)]
pub struct OpenHandleTable {
    handles: Slab<OpenHandle>,
    generations: Vec<u32>,
    max_handles: usize,
    idle_timeout: Duration,
}

impl OpenHandleTable {
    pub fn new(max_handles: usize, idle_timeout: Duration) -> Result<Self, VfsError> {
        if max_handles == 0 || max_handles > u32::MAX as usize {
            return Err(VfsError::InvalidInput(
                "handle limit must be in 1..=u32::MAX",
            ));
        }
        Ok(Self {
            handles: Slab::with_capacity(max_handles.min(1024)),
            generations: Vec::new(),
            max_handles,
            idle_timeout,
        })
    }

    pub fn insert(&mut self, mut handle: OpenHandle) -> Result<OpenHandleId, VfsError> {
        if self.handles.len() >= self.max_handles {
            return Err(VfsError::TooManyHandles);
        }
        let now = Instant::now();
        handle.opened_at = now;
        handle.last_accessed_at = now;
        let index = self.handles.insert(handle);
        if index >= self.generations.len() {
            self.generations.resize(index + 1, 0);
        }
        Ok(OpenHandleId(pack(index, self.generations[index])))
    }

    pub fn get_mut(&mut self, id: OpenHandleId) -> Result<&mut OpenHandle, VfsError> {
        let (index, generation) = unpack(id.0);
        if self.generations.get(index).copied() != Some(generation) {
            return Err(VfsError::BadHandle);
        }
        let handle = self.handles.get_mut(index).ok_or(VfsError::BadHandle)?;
        handle.last_accessed_at = Instant::now();
        Ok(handle)
    }

    pub fn remove(&mut self, id: OpenHandleId) -> Result<OpenHandle, VfsError> {
        let (index, generation) = unpack(id.0);
        if self.generations.get(index).copied() != Some(generation) || !self.handles.contains(index)
        {
            return Err(VfsError::BadHandle);
        }
        let handle = self.handles.remove(index);
        self.generations[index] = self.generations[index].wrapping_add(1);
        Ok(handle)
    }

    pub fn expired_ids(&self, now: Instant) -> Vec<OpenHandleId> {
        self.handles
            .iter()
            .filter(|(_, handle)| {
                now.saturating_duration_since(handle.last_accessed_at) >= self.idle_timeout
            })
            .map(|(index, _)| OpenHandleId(pack(index, self.generations[index])))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    pub fn max_handles(&self) -> usize {
        self.max_handles
    }
}

fn pack(index: usize, generation: u32) -> u64 {
    (u64::from(generation) << 32) | index as u64
}

fn unpack(id: u64) -> (usize, u32) {
    (id as u32 as usize, (id >> 32) as u32)
}
