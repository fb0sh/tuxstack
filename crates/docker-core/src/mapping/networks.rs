//! Mapping for network DTOs.

use bollard::models::{
    Ipam, IpamConfig, Network as BollardNetwork, NetworkCreateRequest, NetworkCreateResponse,
    NetworkInspect as BollardNetworkInspect,
};

use crate::models::{
    CreateNetworkOptions, CreateNetworkResult, NetworkContainer, NetworkDetail, NetworkIpam,
    NetworkSubnet, NetworkSummary,
};

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn first_subnet(ipam: Option<&Ipam>) -> (Option<String>, Option<String>) {
    ipam.and_then(|value| value.config.as_ref())
        .and_then(|configs| configs.iter().find(|config| config.subnet.is_some()))
        .map(|config| (config.subnet.clone(), config.gateway.clone()))
        .unwrap_or_default()
}

fn map_ipam(ipam: Option<Ipam>) -> NetworkIpam {
    let Some(ipam) = ipam else {
        return NetworkIpam::default();
    };
    NetworkIpam {
        driver: ipam.driver,
        options: ipam.options.unwrap_or_default().into_iter().collect(),
        subnets: ipam
            .config
            .unwrap_or_default()
            .into_iter()
            .filter_map(|config| {
                config.subnet.map(|subnet| NetworkSubnet {
                    subnet,
                    gateway: config.gateway,
                    ip_range: config.ip_range,
                    auxiliary_addresses: config
                        .auxiliary_addresses
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                })
            })
            .collect(),
    }
}

/// Map a network list entry without issuing an inspect request.
pub(crate) fn map_network_summary(network: BollardNetwork) -> NetworkSummary {
    let id = network.id.unwrap_or_default();
    let (subnet, gateway) = first_subnet(network.ipam.as_ref());
    NetworkSummary {
        short_id: short_id(&id),
        id,
        name: network.name.unwrap_or_default(),
        driver: network.driver.unwrap_or_default(),
        scope: network.scope.unwrap_or_default(),
        created_at: network.created,
        subnet,
        gateway,
        internal: network.internal.unwrap_or(false),
        attachable: network.attachable.unwrap_or(false),
        ingress: network.ingress.unwrap_or(false),
        ipv4: network.enable_ipv4.unwrap_or(true),
        ipv6: network.enable_ipv6.unwrap_or(false),
        labels: network.labels.unwrap_or_default().into_iter().collect(),
    }
}

/// Map a network inspect response into the complete domain detail model.
pub(crate) fn map_network_detail(inspect: BollardNetworkInspect) -> NetworkDetail {
    let id = inspect.id.unwrap_or_default();
    let (subnet, gateway) = first_subnet(inspect.ipam.as_ref());
    let internal = inspect.internal.unwrap_or(false);
    let attachable = inspect.attachable.unwrap_or(false);
    let ingress = inspect.ingress.unwrap_or(false);
    let summary = NetworkSummary {
        short_id: short_id(&id),
        id,
        name: inspect.name.unwrap_or_default(),
        driver: inspect.driver.unwrap_or_default(),
        scope: inspect.scope.unwrap_or_default(),
        created_at: inspect.created,
        subnet,
        gateway,
        internal,
        attachable,
        ingress,
        ipv4: inspect.enable_ipv4.unwrap_or(true),
        ipv6: inspect.enable_ipv6.unwrap_or(false),
        labels: inspect.labels.unwrap_or_default().into_iter().collect(),
    };
    let mut containers: Vec<_> = inspect
        .containers
        .unwrap_or_default()
        .into_iter()
        .map(|(id, endpoint)| NetworkContainer {
            short_id: short_id(&id),
            id,
            name: endpoint.name.unwrap_or_default(),
            endpoint_id: endpoint.endpoint_id.unwrap_or_default(),
            ipv4_address: endpoint.ipv4_address,
            ipv6_address: endpoint.ipv6_address,
            mac_address: endpoint.mac_address,
        })
        .collect();
    containers.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));

    NetworkDetail {
        internal,
        attachable,
        ingress,
        options: inspect.options.unwrap_or_default().into_iter().collect(),
        ipam: map_ipam(inspect.ipam),
        containers,
        summary,
    }
}

pub(crate) fn create_network_request(options: CreateNetworkOptions) -> NetworkCreateRequest {
    let ipam = (options.subnet.is_some() || options.gateway.is_some()).then(|| Ipam {
        config: Some(vec![IpamConfig {
            subnet: options.subnet,
            gateway: options.gateway,
            ..Default::default()
        }]),
        ..Default::default()
    });
    NetworkCreateRequest {
        name: options.name,
        driver: Some(options.driver),
        internal: Some(options.internal),
        attachable: Some(options.attachable),
        enable_ipv6: Some(options.ipv6),
        ipam,
        labels: (!options.labels.is_empty()).then(|| options.labels.into_iter().collect()),
        options: (!options.options.is_empty()).then(|| options.options.into_iter().collect()),
        ..Default::default()
    }
}

pub(crate) fn map_create_network_result(response: NetworkCreateResponse) -> CreateNetworkResult {
    CreateNetworkResult {
        id: response.id,
        warning: (!response.warning.is_empty()).then_some(response.warning),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bollard::models::{EndpointResource, Network as BN, NetworkInspect as BNI};
    use chrono::{TimeZone, Utc};

    use super::*;

    fn ipam() -> Ipam {
        Ipam {
            driver: Some("default".into()),
            options: Some(
                [("com.example.ipam".into(), "enabled".into())]
                    .into_iter()
                    .collect(),
            ),
            config: Some(vec![
                IpamConfig {
                    subnet: Some("172.20.0.0/16".into()),
                    gateway: Some("172.20.0.1".into()),
                    ip_range: Some("172.20.10.0/24".into()),
                    auxiliary_addresses: Some(
                        [("router".into(), "172.20.0.2".into())]
                            .into_iter()
                            .collect(),
                    ),
                },
                IpamConfig {
                    subnet: Some("fd00:20::/64".into()),
                    gateway: Some("fd00:20::1".into()),
                    ..Default::default()
                },
            ]),
        }
    }

    #[test]
    fn summary_maps_typed_fields_from_list_response() {
        let created = Utc.with_ymd_and_hms(2026, 7, 22, 10, 30, 0).unwrap();
        let mapped = map_network_summary(BN {
            name: Some("bridge".into()),
            id: Some("1234567890abcdef".into()),
            created: Some(created),
            driver: Some("bridge".into()),
            scope: Some("local".into()),
            ipam: Some(ipam()),
            internal: Some(true),
            attachable: Some(false),
            ingress: Some(false),
            enable_ipv4: Some(true),
            enable_ipv6: Some(true),
            labels: Some([("app".into(), "web".into())].into_iter().collect()),
            ..Default::default()
        });
        assert_eq!(mapped.short_id, "1234567890ab");
        assert_eq!(mapped.created_at, Some(created));
        assert_eq!(mapped.subnet.as_deref(), Some("172.20.0.0/16"));
        assert_eq!(mapped.gateway.as_deref(), Some("172.20.0.1"));
        assert_eq!(mapped.labels.get("app").map(String::as_str), Some("web"));
        assert!(mapped.internal && mapped.ipv4 && mapped.ipv6);
    }

    #[test]
    fn empty_network_does_not_panic() {
        let mapped = map_network_summary(BN::default());
        assert_eq!(mapped.name, "");
        assert_eq!(mapped.short_id, "");
        assert!(mapped.ipv4);
        assert!(!mapped.internal);
    }

    #[test]
    fn detail_maps_all_subnets_options_labels_and_exact_endpoint_fields() {
        let mapped = map_network_detail(BNI {
            id: Some("network-id".into()),
            name: Some("custom".into()),
            ipam: Some(ipam()),
            options: Some(
                [("com.example.option".into(), "true".into())]
                    .into_iter()
                    .collect(),
            ),
            labels: Some([("z-label".into(), "value".into())].into_iter().collect()),
            containers: Some(
                [(
                    "exact-container-id".into(),
                    EndpointResource {
                        name: Some("web".into()),
                        endpoint_id: Some("exact-endpoint-id".into()),
                        ipv4_address: Some("172.20.0.2/16".into()),
                        ipv6_address: Some("fd00:20::2/64".into()),
                        mac_address: Some("02:42:ac:14:00:02".into()),
                    },
                )]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        });
        assert_eq!(mapped.ipam.driver.as_deref(), Some("default"));
        assert_eq!(
            mapped
                .ipam
                .options
                .get("com.example.ipam")
                .map(String::as_str),
            Some("enabled")
        );
        assert_eq!(mapped.ipam.subnets.len(), 2);
        assert_eq!(
            mapped.ipam.subnets[0].ip_range.as_deref(),
            Some("172.20.10.0/24")
        );
        assert_eq!(
            mapped.ipam.subnets[0]
                .auxiliary_addresses
                .get("router")
                .map(String::as_str),
            Some("172.20.0.2")
        );
        assert_eq!(mapped.ipam.subnets[1].subnet, "fd00:20::/64");
        assert_eq!(
            mapped.ipam.subnets[1].gateway.as_deref(),
            Some("fd00:20::1")
        );
        assert_eq!(
            mapped.options.get("com.example.option").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            mapped.summary.labels.get("z-label").map(String::as_str),
            Some("value")
        );
        let endpoint = &mapped.containers[0];
        assert_eq!(endpoint.id, "exact-container-id");
        assert_eq!(endpoint.short_id, "exact-contai");
        assert_eq!(endpoint.name, "web");
        assert_eq!(endpoint.endpoint_id, "exact-endpoint-id");
        assert_eq!(endpoint.ipv4_address.as_deref(), Some("172.20.0.2/16"));
        assert_eq!(endpoint.ipv6_address.as_deref(), Some("fd00:20::2/64"));
        assert_eq!(endpoint.mac_address.as_deref(), Some("02:42:ac:14:00:02"));
    }

    #[test]
    fn create_options_map_to_bollard_request_and_result() {
        let request = create_network_request(CreateNetworkOptions {
            name: "custom".into(),
            driver: "bridge".into(),
            subnet: Some("172.30.0.0/16".into()),
            gateway: Some("172.30.0.1".into()),
            ipv6: true,
            internal: true,
            attachable: true,
            labels: BTreeMap::from([("app".into(), "test".into())]),
            options: BTreeMap::from([("com.example.option".into(), "yes".into())]),
        });
        assert_eq!(request.name, "custom");
        assert_eq!(request.driver.as_deref(), Some("bridge"));
        assert_eq!(request.enable_ipv6, Some(true));
        let config = request.ipam.unwrap().config.unwrap().remove(0);
        assert_eq!(config.subnet.as_deref(), Some("172.30.0.0/16"));
        assert_eq!(config.gateway.as_deref(), Some("172.30.0.1"));
        assert_eq!(
            request.labels.unwrap().get("app").map(String::as_str),
            Some("test")
        );

        let result = map_create_network_result(NetworkCreateResponse {
            id: "new-id".into(),
            warning: String::new(),
        });
        assert_eq!(result.id, "new-id");
        assert_eq!(result.warning, None);
    }
}
