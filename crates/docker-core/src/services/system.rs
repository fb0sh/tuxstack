//! System information service.

use std::sync::Arc;

use crate::client::DockerClient;
use crate::error::{classify_api_error, DockerError};
use crate::mapping::system::{apply_system_version, map_system_info};
use crate::models::{DockerSystemInfo, OverviewData};

/// System-level service (ping, info, version, overview aggregation).
#[derive(Clone)]
pub struct SystemService {
    client: Arc<DockerClient>,
}

impl SystemService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// Verify the engine is reachable.
    pub async fn ping(&self) -> Result<(), DockerError> {
        self.client.ping().await
    }

    /// Fetch system info with version details merged in.
    pub async fn system_info(&self) -> Result<DockerSystemInfo, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;

        let info = tokio::time::timeout(timeout, docker.info())
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|e| classify_api_error(&e, "system"))?;
        let mut info = map_system_info(info);

        if let Ok(version) = tokio::time::timeout(timeout, docker.version())
            .await
            .map_err(|_| DockerError::OperationTimeout)
            .and_then(|r| r.map_err(|e| classify_api_error(&e, "system")))
        {
            apply_system_version(&mut info, version);
        }

        Ok(info)
    }

    /// Aggregate the data shown on the Overview page.
    pub async fn overview(&self) -> Result<OverviewData, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;

        let info = self.system_info().await?;

        let network_count = tokio::time::timeout(
            timeout,
            docker.list_networks(None::<bollard::query_parameters::ListNetworksOptions>),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "network"))?
        .len() as u64;

        let volume_count = tokio::time::timeout(timeout, docker.list_volumes(None::<bollard::query_parameters::ListVolumesOptions>))
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|e| classify_api_error(&e, "volume"))?
            .volumes
            .map(|v| v.len() as u64)
            .unwrap_or(0);

        Ok(OverviewData {
            system: info,
            networks: network_count,
            volumes: volume_count,
        })
    }
}
