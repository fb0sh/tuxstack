//! Network management service.

use std::sync::Arc;

use bollard::query_parameters::ListNetworksOptions as BollardListNetworksOptions;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};
use crate::mapping::networks::{map_network_detail, map_network_summary};
use crate::models::{NetworkDetail, NetworkSummary};

/// Options for listing networks.
#[derive(Debug, Clone, Default)]
pub struct ListNetworksOptions {
    /// Local search filter on names and IDs.
    pub search: Option<String>,
}

/// Network service backed by the shared Docker client.
#[derive(Clone)]
pub struct NetworkService {
    client: Arc<DockerClient>,
}

impl NetworkService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List networks, applying a local name/ID search filter.
    pub async fn list_networks(
        &self,
        options: &ListNetworksOptions,
    ) -> Result<Vec<NetworkSummary>, DockerError> {
        let docker = self.client.inner().clone();
        let networks = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.list_networks(None::<BollardListNetworksOptions>),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "network"))?;

        let mut mapped: Vec<NetworkSummary> =
            networks.into_iter().map(map_network_summary).collect();

        if let Some(search) = options.search.as_deref().map(str::to_lowercase) {
            mapped.retain(|n| {
                n.name.to_lowercase().contains(&search)
                    || n.id.contains(&search)
                    || n.driver.to_lowercase().contains(&search)
            });
        }

        mapped.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(mapped)
    }

    /// Inspect a single network.
    pub async fn inspect_network(&self, name: &str) -> Result<NetworkDetail, DockerError> {
        let docker = self.client.inner().clone();
        let detail = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.inspect_network(name, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "network"))?;
        Ok(map_network_detail(detail))
    }
}
