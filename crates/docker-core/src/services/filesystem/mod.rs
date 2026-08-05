//! Unified filesystem browsing service.
//!
//! Both image and volume browsing share the same helper binary
//! (`tuxstack-fs-helper`), the same wire protocol, and the same exec
//! infrastructure. This module provides a single entry point for all
//! filesystem operations, with `image_provider` and `volume_provider`
//! handling the session creation differences.

pub mod client;
pub mod error;
pub mod image_provider;
pub mod session;
pub mod types;
pub mod volume_provider;

use std::sync::Arc;

use tuxstack_fs_protocol::FilesystemPathToken;

use crate::client::DockerClient;

use self::types::*;
use error::FilesystemError;
use types::{
    FilesystemEntry, FilesystemSession, HashRequest, ListDirectoryRequest, ListDirectoryResult,
    PreviewRequest, StatRequest,
};

/// The unified filesystem browsing service. Delegates session creation to
/// the image/volume providers and all filesystem operations to the client.
#[derive(Clone)]
pub struct FilesystemService {
    client: Arc<DockerClient>,
    operation_timeout: std::time::Duration,
}

impl FilesystemService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self {
            client,
            operation_timeout: std::time::Duration::from_secs(60),
        }
    }

    pub fn with_timeout(client: Arc<DockerClient>, timeout: std::time::Duration) -> Self {
        Self {
            client,
            operation_timeout: timeout,
        }
    }

    fn docker(&self) -> &bollard::Docker {
        self.client.inner()
    }

    // -----------------------------------------------------------------------
    // Session management
    // -----------------------------------------------------------------------

    /// Create a session for an image.
    pub async fn start_image_session(
        &self,
        image_id: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<FilesystemSession, FilesystemError> {
        image_provider::create_session(
            self.docker(),
            image_id,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    /// Create a session for a volume.
    pub async fn start_volume_session(
        &self,
        volume_name: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<FilesystemSession, FilesystemError> {
        volume_provider::create_session(
            self.docker(),
            volume_name,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    /// Stop a session (force-remove the container).
    pub async fn stop_session(&self, session: &FilesystemSession) -> Result<(), FilesystemError> {
        session::invalidate_session(self.docker(), session).await
    }

    /// Clean up orphaned helper containers.
    pub async fn cleanup_orphan_sessions(&self) -> Result<usize, FilesystemError> {
        session::cleanup_orphan_sessions(self.docker(), "filesystem-helper").await
    }

    // -----------------------------------------------------------------------
    // Filesystem operations
    // -----------------------------------------------------------------------

    pub async fn list_directory(
        &self,
        session: &FilesystemSession,
        request: &ListDirectoryRequest,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<ListDirectoryResult, FilesystemError> {
        client::list_directory(
            self.docker(),
            session,
            request,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    pub async fn stat(
        &self,
        session: &FilesystemSession,
        request: &StatRequest,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<FilesystemEntry, FilesystemError> {
        client::stat(
            self.docker(),
            session,
            request,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    pub async fn preview(
        &self,
        session: &FilesystemSession,
        request: &PreviewRequest,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<PreviewResult, FilesystemError> {
        client::preview(
            self.docker(),
            session,
            request,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    pub async fn hash(
        &self,
        session: &FilesystemSession,
        request: &HashRequest,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<(String, String), FilesystemError> {
        client::hash(
            self.docker(),
            session,
            request,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }

    pub async fn readlink(
        &self,
        session: &FilesystemSession,
        request: &StatRequest,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<FilesystemPathToken, FilesystemError> {
        client::readlink(
            self.docker(),
            session,
            request,
            self.operation_timeout,
            &cancellation,
        )
        .await
    }
}
