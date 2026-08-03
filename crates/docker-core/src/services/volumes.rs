//! Volume management service.

use std::sync::Arc;

use bollard::query_parameters::ListVolumesOptions as BollardListVolumesOptions;

use crate::client::DockerClient;
use crate::error::{classify_api_error, DockerError};
use crate::mapping::volumes::map_volume_summary;
use crate::models::VolumeSummary;

/// Options for listing volumes.
#[derive(Debug, Clone, Default)]
pub struct ListVolumesOptions {
    /// Local search filter on names.
    pub search: Option<String>,
}

/// Volume service backed by the shared Docker client.
#[derive(Clone)]
pub struct VolumeService {
    client: Arc<DockerClient>,
}

impl VolumeService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List volumes, applying a local name search filter.
    pub async fn list_volumes(
        &self,
        options: &ListVolumesOptions,
    ) -> Result<Vec<VolumeSummary>, DockerError> {
        let docker = self.client.inner().clone();
        let response = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.list_volumes(None::<BollardListVolumesOptions>),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "volume"))?;

        let mut mapped: Vec<VolumeSummary> = response
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(map_volume_summary)
            .collect();

        if let Some(search) = options.search.as_deref().map(str::to_lowercase) {
            mapped.retain(|v| v.name.to_lowercase().contains(&search));
        }

        mapped.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(mapped)
    }

    /// Remove a volume. Volumes may be in use; the engine returns the
    /// real conflict error in that case.
    pub async fn remove_volume(&self, name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_volume(name, None::<bollard::query_parameters::RemoveVolumeOptions>),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "volume"))
    }
}
