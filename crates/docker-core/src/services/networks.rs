//! Network management service.

use std::sync::Arc;

use bollard::query_parameters::ListNetworksOptions as BollardListNetworksOptions;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_network_api_error};
use crate::mapping::networks::{
    create_network_request, map_create_network_result, map_network_detail, map_network_summary,
};
use crate::models::{CreateNetworkOptions, CreateNetworkResult, NetworkDetail, NetworkSummary};

/// Options for listing networks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListNetworksOptions {
    /// Local search over names, IDs, drivers, IPAM, and labels.
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

    /// List networks with exactly one Docker request. Search and sorting are
    /// local; no inspect-per-network request is performed.
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
        .map_err(|error| classify_network_api_error(&error, "list"))?;

        let mut mapped: Vec<NetworkSummary> =
            networks.into_iter().map(map_network_summary).collect();
        apply_search(&mut mapped, options.search.as_deref());
        mapped.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok(mapped)
    }

    /// Inspect only the selected network.
    pub async fn inspect_network(&self, id: &str) -> Result<NetworkDetail, DockerError> {
        let docker = self.client.inner().clone();
        let detail = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.inspect_network(id, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_network_api_error(&error, "inspect"))?;
        Ok(map_network_detail(detail))
    }

    /// Create a network and return Docker's generated ID and warning.
    pub async fn create_network(
        &self,
        mut options: CreateNetworkOptions,
    ) -> Result<CreateNetworkResult, DockerError> {
        options.name = options.name.trim().to_string();
        options.driver = options.driver.trim().to_string();
        if options.name.is_empty() {
            return Err(DockerError::InvalidNetworkConfig(
                "network name must not be empty".to_string(),
            ));
        }
        if options.driver.is_empty() {
            return Err(DockerError::InvalidNetworkConfig(
                "network driver must not be empty".to_string(),
            ));
        }
        if options.gateway.is_some() && options.subnet.is_none() {
            return Err(DockerError::InvalidNetworkConfig(
                "a gateway requires a subnet".to_string(),
            ));
        }

        let docker = self.client.inner().clone();
        let response = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.create_network(create_network_request(options)),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_network_api_error(&error, "create"))?;
        Ok(map_create_network_result(response))
    }

    /// Remove a network. Docker refuses networks with active endpoints; this
    /// method does not disconnect or remove containers automatically.
    pub async fn remove_network(&self, id: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_network(id),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_network_api_error(&error, "remove"))
    }
}

fn apply_search(networks: &mut Vec<NetworkSummary>, search: Option<&str>) {
    let Some(search) = search.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let search = search.to_ascii_lowercase();
    networks.retain(|network| {
        network.id.to_ascii_lowercase().contains(&search)
            || network.short_id.to_ascii_lowercase().contains(&search)
            || network.name.to_ascii_lowercase().contains(&search)
            || network.driver.to_ascii_lowercase().contains(&search)
            || network
                .subnet
                .as_deref()
                .is_some_and(|value| value.to_ascii_lowercase().contains(&search))
            || network
                .gateway
                .as_deref()
                .is_some_and(|value| value.to_ascii_lowercase().contains(&search))
            || network.labels.iter().any(|(key, value)| {
                key.to_ascii_lowercase().contains(&search)
                    || value.to_ascii_lowercase().contains(&search)
            })
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn summary() -> NetworkSummary {
        NetworkSummary {
            id: "1234567890abcdef".into(),
            short_id: "1234567890ab".into(),
            name: "Example-Network".into(),
            driver: "bridge".into(),
            scope: "local".into(),
            created_at: None,
            subnet: Some("172.30.0.0/16".into()),
            gateway: Some("172.30.0.1".into()),
            internal: false,
            attachable: false,
            ingress: false,
            ipv4: true,
            ipv6: false,
            labels: BTreeMap::from([("com.example.role".into(), "Frontend".into())]),
        }
    }

    #[test]
    fn search_is_local_trimmed_case_insensitive_and_covers_fields() {
        for query in [
            " EXAMPLE ",
            "1234567890AB",
            "BRIDGE",
            "172.30.0.0",
            "172.30.0.1",
            "ROLE",
            "FRONTEND",
        ] {
            let mut networks = vec![summary()];
            apply_search(&mut networks, Some(query));
            assert_eq!(networks.len(), 1, "query {query}");
        }
    }

    #[test]
    fn search_no_match_filters_network() {
        let mut networks = vec![summary()];
        apply_search(&mut networks, Some("missing"));
        assert!(networks.is_empty());
    }
}
