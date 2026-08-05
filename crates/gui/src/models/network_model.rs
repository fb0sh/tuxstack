//! Pure Docker network view models used by the Qt bridge.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use tuxstack_domain::{NetworkContainer, NetworkDetail, NetworkSubnet, NetworkSummary};

/// One row in the network list. This type deliberately contains no Qt values.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkRow {
    /// The complete Docker network ID. Never substitute the display short ID.
    pub network_id: String,
    pub short_id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub subnet: String,
    pub gateway: String,
    pub secondary_text: String,
    pub created_at: Option<DateTime<Utc>>,
    pub created_text: String,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub ipv4: bool,
    pub ipv6: bool,
    pub labels: BTreeMap<String, String>,
}

impl From<&NetworkSummary> for NetworkRow {
    fn from(summary: &NetworkSummary) -> Self {
        let subnet = summary.subnet.clone().unwrap_or_default();
        let gateway = summary.gateway.clone().unwrap_or_default();
        let secondary_text = if subnet.is_empty() {
            value_or_dash(Some(&summary.driver))
        } else {
            subnet.clone()
        };

        Self {
            network_id: summary.id.clone(),
            short_id: summary.short_id.clone(),
            name: summary.name.clone(),
            driver: summary.driver.clone(),
            scope: summary.scope.clone(),
            subnet,
            gateway,
            secondary_text,
            created_at: summary.created_at,
            created_text: summary.created_at.map(relative_time).unwrap_or_else(dash),
            internal: summary.internal,
            attachable: summary.attachable,
            ingress: summary.ingress,
            ipv4: summary.ipv4,
            ipv6: summary.ipv6,
            labels: summary.labels.clone(),
        }
    }
}

/// A structured key/value entry. JSON is never passed across the Qt boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkKeyValueRow {
    pub key: String,
    pub value: String,
}

/// One IPAM subnet, including every auxiliary address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSubnetView {
    pub subnet: String,
    pub gateway: String,
    pub ip_range: String,
    pub auxiliary_addresses: Vec<NetworkKeyValueRow>,
}

impl From<&NetworkSubnet> for NetworkSubnetView {
    fn from(subnet: &NetworkSubnet) -> Self {
        Self {
            subnet: value_or_dash(Some(&subnet.subnet)),
            gateway: value_or_dash(subnet.gateway.as_deref()),
            ip_range: value_or_dash(subnet.ip_range.as_deref()),
            auxiliary_addresses: sorted_pairs(&subnet.auxiliary_addresses),
        }
    }
}

/// A container attached to the selected network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkContainerView {
    pub container_id: String,
    pub short_id: String,
    pub name: String,
    pub endpoint_id: String,
    pub ipv4_address: String,
    pub ipv6_address: String,
    pub mac_address: String,
}

impl From<&NetworkContainer> for NetworkContainerView {
    fn from(container: &NetworkContainer) -> Self {
        Self {
            container_id: container.id.clone(),
            short_id: container.short_id.clone(),
            name: container.name.clone(),
            endpoint_id: value_or_dash(Some(&container.endpoint_id)),
            ipv4_address: value_or_dash(container.ipv4_address.as_deref()),
            ipv6_address: value_or_dash(container.ipv6_address.as_deref()),
            mac_address: value_or_dash(container.mac_address.as_deref()),
        }
    }
}

/// Scalar and structured values for the permanent network detail panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkDetailView {
    pub network_id: String,
    pub short_id: String,
    pub name: String,
    pub created_text: String,
    pub created_full_text: String,
    pub driver: String,
    pub scope: String,
    pub subnet: String,
    pub gateway: String,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub ipv4: bool,
    pub ipv6: bool,
    pub ipam_driver: String,
    pub options: Vec<NetworkKeyValueRow>,
    pub ipam_options: Vec<NetworkKeyValueRow>,
    pub labels: Vec<NetworkKeyValueRow>,
    pub subnets: Vec<NetworkSubnetView>,
    pub containers: Vec<NetworkContainerView>,
}

impl From<&NetworkDetail> for NetworkDetailView {
    fn from(detail: &NetworkDetail) -> Self {
        let summary = &detail.summary;
        let mut subnets: Vec<NetworkSubnetView> =
            detail.ipam.subnets.iter().map(Into::into).collect();
        // Docker normally preserves IPAM order. A deterministic tie-break keeps
        // equivalent daemon responses stable without dropping any subnet.
        subnets.sort_by(|left, right| {
            left.subnet
                .to_lowercase()
                .cmp(&right.subnet.to_lowercase())
                .then_with(|| left.gateway.cmp(&right.gateway))
        });
        let mut containers: Vec<NetworkContainerView> =
            detail.containers.iter().map(Into::into).collect();
        containers.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.container_id.cmp(&right.container_id))
        });

        Self {
            network_id: summary.id.clone(),
            short_id: summary.short_id.clone(),
            name: summary.name.clone(),
            created_text: summary.created_at.map(relative_time).unwrap_or_else(dash),
            created_full_text: summary.created_at.map(full_utc_time).unwrap_or_else(dash),
            driver: value_or_dash(Some(&summary.driver)),
            scope: value_or_dash(Some(&summary.scope)),
            subnet: value_or_dash(summary.subnet.as_deref()),
            gateway: value_or_dash(summary.gateway.as_deref()),
            internal: summary.internal,
            attachable: summary.attachable,
            ingress: summary.ingress,
            ipv4: summary.ipv4,
            ipv6: summary.ipv6,
            ipam_driver: value_or_dash(detail.ipam.driver.as_deref()),
            options: sorted_pairs(&detail.options),
            ipam_options: sorted_pairs(&detail.ipam.options),
            labels: sorted_pairs(&summary.labels),
            subnets,
            containers,
        }
    }
}

fn sorted_pairs(values: &BTreeMap<String, String>) -> Vec<NetworkKeyValueRow> {
    let mut rows: Vec<_> = values
        .iter()
        .map(|(key, value)| NetworkKeyValueRow {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();
    rows.sort_by(|left, right| {
        left.key
            .to_lowercase()
            .cmp(&right.key.to_lowercase())
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
}

fn relative_time(created: DateTime<Utc>) -> String {
    relative_time_at(created, Utc::now())
}

fn relative_time_at(created: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let age = now.signed_duration_since(created);
    if age.num_seconds() < 0 {
        return full_utc_time(created);
    }
    if age.num_seconds() < 60 {
        "just now".to_string()
    } else if age.num_minutes() < 60 {
        relative_unit(age.num_minutes(), "minute")
    } else if age.num_hours() < 24 {
        relative_unit(age.num_hours(), "hour")
    } else if age.num_days() < 14 {
        relative_unit(age.num_days(), "day")
    } else if age.num_days() < 60 {
        relative_unit(age.num_weeks(), "week")
    } else {
        created.format("%Y-%m-%d").to_string()
    }
}

fn relative_unit(value: i64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{suffix} ago")
}

fn full_utc_time(created: DateTime<Utc>) -> String {
    created.format("%b %-d, %Y %H:%M UTC").to_string()
}

fn value_or_dash(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn dash() -> String {
    "—".to_string()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use tuxstack_domain::NetworkIpam;

    use super::*;

    fn summary() -> NetworkSummary {
        NetworkSummary {
            id: "0123456789abcdef".into(),
            short_id: "0123456789ab".into(),
            name: "frontend".into(),
            driver: "bridge".into(),
            scope: "local".into(),
            created_at: Some(Utc.with_ymd_and_hms(2026, 7, 22, 12, 40, 33).unwrap()),
            subnet: Some("172.20.0.0/16".into()),
            gateway: Some("172.20.0.1".into()),
            internal: true,
            attachable: false,
            ingress: false,
            ipv4: true,
            ipv6: true,
            labels: BTreeMap::from([
                ("z.example".into(), "last".into()),
                ("A.example".into(), "first".into()),
            ]),
        }
    }

    #[test]
    fn list_row_keeps_full_id_flags_and_searchable_data() {
        let row = NetworkRow::from(&summary());
        assert_eq!(row.network_id, "0123456789abcdef");
        assert_eq!(row.short_id, "0123456789ab");
        assert_eq!(row.secondary_text, "172.20.0.0/16");
        assert_eq!(row.gateway, "172.20.0.1");
        assert!(row.internal);
        assert!(!row.attachable);
        assert!(row.ipv4);
        assert!(row.ipv6);
        assert_eq!(row.labels["z.example"], "last");
    }

    #[test]
    fn list_row_falls_back_to_driver_without_subnet() {
        let mut source = summary();
        source.subnet = None;
        source.gateway = None;
        let row = NetworkRow::from(&source);
        assert_eq!(row.secondary_text, "bridge");
        assert!(row.subnet.is_empty());
        assert!(row.gateway.is_empty());
    }

    #[test]
    fn detail_is_structured_sorted_and_preserves_all_ipam_data() {
        let detail = NetworkDetail {
            summary: summary(),
            internal: true,
            attachable: false,
            ingress: false,
            options: BTreeMap::from([
                ("z.option".into(), "z".into()),
                ("a.option".into(), "a".into()),
            ]),
            ipam: NetworkIpam {
                driver: Some("default".into()),
                options: BTreeMap::from([("b.ipam".into(), "two".into())]),
                subnets: vec![
                    NetworkSubnet {
                        subnet: "fd00::/64".into(),
                        gateway: Some("fd00::1".into()),
                        ip_range: None,
                        auxiliary_addresses: BTreeMap::new(),
                    },
                    NetworkSubnet {
                        subnet: "172.20.0.0/16".into(),
                        gateway: Some("172.20.0.1".into()),
                        ip_range: Some("172.20.5.0/24".into()),
                        auxiliary_addresses: BTreeMap::from([
                            ("router".into(), "172.20.0.2".into()),
                            ("dns".into(), "172.20.0.3".into()),
                        ]),
                    },
                ],
            },
            containers: vec![
                NetworkContainer {
                    id: "container-z-full".into(),
                    short_id: "container-z".into(),
                    name: "web".into(),
                    endpoint_id: "endpoint-z".into(),
                    ipv4_address: Some("172.20.0.5/16".into()),
                    ipv6_address: None,
                    mac_address: Some("02:42:ac:14:00:05".into()),
                },
                NetworkContainer {
                    id: "container-a-full".into(),
                    short_id: "container-a".into(),
                    name: "api".into(),
                    endpoint_id: "endpoint-a".into(),
                    ipv4_address: Some("172.20.0.4/16".into()),
                    ipv6_address: Some("fd00::4/64".into()),
                    mac_address: None,
                },
            ],
        };

        let view = NetworkDetailView::from(&detail);
        assert_eq!(view.network_id, "0123456789abcdef");
        assert_eq!(view.created_full_text, "Jul 22, 2026 12:40 UTC");
        assert_eq!(view.options[0].key, "a.option");
        assert_eq!(view.labels[0].key, "A.example");
        assert_eq!(view.ipam_driver, "default");
        assert_eq!(view.ipam_options[0].key, "b.ipam");
        assert_eq!(view.subnets.len(), 2);
        assert_eq!(view.subnets[0].subnet, "172.20.0.0/16");
        assert_eq!(view.subnets[0].ip_range, "172.20.5.0/24");
        assert_eq!(view.subnets[0].auxiliary_addresses[0].key, "dns");
        assert_eq!(view.containers.len(), 2);
        assert_eq!(view.containers[0].name, "api");
        assert_eq!(view.containers[0].container_id, "container-a-full");
        assert_eq!(view.containers[0].mac_address, "—");
        assert_eq!(view.containers[1].endpoint_id, "endpoint-z");
    }

    #[test]
    fn missing_optional_detail_values_are_safe_placeholders() {
        let mut source = summary();
        source.created_at = None;
        source.subnet = None;
        source.gateway = None;
        let detail = NetworkDetail {
            summary: source,
            internal: true,
            attachable: false,
            ingress: false,
            options: BTreeMap::new(),
            ipam: NetworkIpam {
                driver: None,
                options: BTreeMap::new(),
                subnets: vec![],
            },
            containers: vec![],
        };
        let view = NetworkDetailView::from(&detail);
        assert_eq!(view.created_text, "—");
        assert_eq!(view.created_full_text, "—");
        assert_eq!(view.subnet, "—");
        assert_eq!(view.gateway, "—");
        assert_eq!(view.ipam_driver, "—");
        assert!(view.options.is_empty());
        assert!(view.labels.len() == 2);
        assert!(view.subnets.is_empty());
        assert!(view.containers.is_empty());
    }

    #[test]
    fn relative_dates_cover_singular_plural_future_and_absolute_ranges() {
        let now = Utc.with_ymd_and_hms(2026, 7, 25, 12, 40, 0).unwrap();
        assert_eq!(
            relative_time_at(now - Duration::minutes(1), now),
            "1 minute ago"
        );
        assert_eq!(
            relative_time_at(now - Duration::hours(2), now),
            "2 hours ago"
        );
        assert_eq!(relative_time_at(now - Duration::days(3), now), "3 days ago");
        assert_eq!(
            relative_time_at(now - Duration::days(20), now),
            "2 weeks ago"
        );
        assert_eq!(
            relative_time_at(now - Duration::days(70), now),
            "2026-05-16"
        );
        assert_eq!(
            relative_time_at(now + Duration::minutes(1), now),
            "Jul 25, 2026 12:41 UTC"
        );
    }
}
