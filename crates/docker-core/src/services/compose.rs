//! Compose service.
//!
//! Compose support is planned for a future release. This service is an
//! honest placeholder that reports the planned status; it never invents
//! fake projects or runs shell commands.

use std::sync::Arc;

use crate::client::DockerClient;
use crate::error::DockerError;
use crate::models::ComposeProject;

/// Compose service (planned).
#[derive(Clone)]
pub struct ComposeService {
    _client: Arc<DockerClient>,
}

impl ComposeService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { _client: client }
    }

    /// Compose support is planned but not yet implemented.
    pub async fn list_projects(&self) -> Result<Vec<ComposeProject>, DockerError> {
        Err(DockerError::Internal(
            "Docker Compose support is planned but not implemented yet".to_string(),
        ))
    }
}
