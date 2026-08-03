//! Mapping for network DTOs.

use bollard::models::{Network as BollardNetwork, NetworkInspect as BollardNetworkInspect};

use crate::models::{NetworkContainer, NetworkDetail, NetworkSummary};

/// Map a bollard network list entry into the domain model.
pub fn map_network_summary(network: BollardNetwork) -> NetworkSummary {
    NetworkSummary {
        id: network.id.unwrap_or_default(),
        name: network.name.unwrap_or_default(),
        driver: network.driver.unwrap_or_default(),
        scope: network.scope.unwrap_or_default(),
        internal: network.internal.unwrap_or(false),
        attachable: network.attachable.unwrap_or(false),
        ingress: network.ingress.unwrap_or(false),
        ipv6: network.enable_ipv6.unwrap_or(false),
        labels: network.labels.unwrap_or_default().into_iter().collect(),
    }
}

/// Map a bollard network inspect response into the domain detail model.
pub fn map_network_detail(inspect: BollardNetworkInspect) -> NetworkDetail {
    let ipam = inspect.ipam.as_ref();
    let config = ipam
        .and_then(|i| i.config.as_ref())
        .and_then(|c| c.first());

    NetworkDetail {
        summary: NetworkSummary {
            id: inspect.id.clone().unwrap_or_default(),
            name: inspect.name.clone().unwrap_or_default(),
            driver: inspect.driver.clone().unwrap_or_default(),
            scope: inspect.scope.clone().unwrap_or_default(),
            internal: inspect.internal.unwrap_or(false),
            attachable: inspect.attachable.unwrap_or(false),
            ingress: inspect.ingress.unwrap_or(false),
            ipv6: inspect.enable_ipv6.unwrap_or(false),
            labels: inspect.labels.clone().unwrap_or_default().into_iter().collect(),
        },
        subnet: config.and_then(|c| c.subnet.clone()),
        gateway: config.and_then(|c| c.gateway.clone()),
        containers: inspect
            .containers
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, endpoint)| NetworkContainer {
                name,
                ipv4: endpoint.ipv4_address.clone(),
                ipv6: endpoint.ipv6_address.clone(),
                mac: endpoint.mac_address.clone(),
            })
            .collect(),
        options: inspect.options.clone().unwrap_or_default().into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        EndpointResource, Ipam, IpamConfig, Network as BN, NetworkInspect as BNI,
    };

    #[test]
    fn maps_summary_fields() {
        let n = BN {
            name: Some("bridge".into()),
            id: Some("net123".into()),
            driver: Some("bridge".into()),
            scope: Some("local".into()),
            internal: Some(true),
            attachable: Some(false),
            ingress: Some(false),
            enable_ipv6: Some(true),
            ..Default::default()
        };
        let mapped = map_network_summary(n);
        assert_eq!(mapped.name, "bridge");
        assert!(mapped.internal);
        assert!(mapped.ipv6);
        assert!(!mapped.attachable);
    }

    #[test]
    fn empty_network_does_not_panic() {
        let mapped = map_network_summary(BN::default());
        assert_eq!(mapped.name, "");
        assert!(!mapped.internal);
    }

    #[test]
    fn maps_detail_with_ipam_and_containers() {
        let inspect = BNI {
            name: Some("custom".into()),
            ipam: Some(Ipam {
                config: Some(vec![IpamConfig {
                    subnet: Some("172.20.0.0/16".into()),
                    gateway: Some("172.20.0.1".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            containers: Some(
                vec![(
                    "web".into(),
                    EndpointResource {
                        ipv4_address: Some("172.20.0.2".into()),
                        mac_address: Some("02:42:ac:14:00:02".into()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };
        let mapped = map_network_detail(inspect);
        assert_eq!(mapped.subnet.as_deref(), Some("172.20.0.0/16"));
        assert_eq!(mapped.gateway.as_deref(), Some("172.20.0.1"));
        assert_eq!(mapped.containers.len(), 1);
        assert_eq!(mapped.containers[0].name, "web");
        assert_eq!(
            mapped.containers[0].ipv4.as_deref(),
            Some("172.20.0.2")
        );
    }
}
