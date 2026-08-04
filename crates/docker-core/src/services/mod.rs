//! Application services that operate on the Docker Engine.
//!
//! Each service owns a shared `Arc<DockerClient>` and returns domain
//! models only. There is deliberately no generic backend trait: the
//! current version targets Docker only.

pub mod compose;
pub mod container_files;
pub mod container_terminal;
pub mod containers;
pub mod filesystem;
pub mod images;
pub mod networks;
pub mod system;
pub mod volumes;

pub use compose::*;
pub use container_files::*;
pub use container_terminal::*;
pub use containers::*;
pub use filesystem::FilesystemService;
pub use images::*;
pub use networks::*;
pub use system::*;
pub use volumes::*;

use std::sync::Arc;

use crate::client::DockerClient;

/// Aggregate entry point exposing every service.
///
/// All services share the same `Arc<DockerClient>`.
#[derive(Clone)]
pub struct DockerServices {
    pub system: SystemService,
    pub containers: ContainerService,
    pub images: ImageService,
    pub networks: NetworkService,
    pub volumes: VolumeService,
    pub compose: ComposeService,
    pub container_files: ContainerFilesystemService,
    pub container_terminal: ContainerTerminalService,
    pub filesystem: FilesystemService,
}

impl DockerServices {
    /// Build services from a connected client.
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self {
            system: SystemService::new(client.clone()),
            containers: ContainerService::new(client.clone()),
            images: ImageService::new(client.clone()),
            networks: NetworkService::new(client.clone()),
            volumes: VolumeService::new(client.clone()),
            compose: ComposeService::new(client.clone()),
            container_files: ContainerFilesystemService::new(client.clone()),
            container_terminal: ContainerTerminalService::new(client.clone()),
            filesystem: FilesystemService::new(client),
        }
    }

    /// The shared client backing every service (used by the event monitor).
    pub fn client(&self) -> Arc<DockerClient> {
        self.system.client()
    }
}
