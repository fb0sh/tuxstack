//! Network domain models.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A network returned by Docker's list endpoint.
///
/// IPAM fields come directly from that response; listing networks never
/// performs an inspect request per item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkSummary {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub created_at: Option<DateTime<Utc>>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub ipv4: bool,
    pub ipv6: bool,
    pub labels: BTreeMap<String, String>,
}

/// Fully inspected network information. No Bollard DTO is exposed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkDetail {
    pub summary: NetworkSummary,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub options: BTreeMap<String, String>,
    pub ipam: NetworkIpam,
    pub containers: Vec<NetworkContainer>,
}

/// Address-management configuration for a network.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkIpam {
    pub driver: Option<String>,
    pub options: BTreeMap<String, String>,
    pub subnets: Vec<NetworkSubnet>,
}

/// Compatibility spelling matching Docker's IPAM acronym.
#[allow(clippy::upper_case_acronyms)]
pub type NetworkIPAM = NetworkIpam;

/// One configured IPAM subnet and its optional gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkSubnet {
    pub subnet: String,
    pub gateway: Option<String>,
    pub ip_range: Option<String>,
    pub auxiliary_addresses: BTreeMap<String, String>,
}

/// An endpoint attached to an inspected network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkContainer {
    /// Exact key from Docker's `Containers` map (the container ID).
    pub id: String,
    pub short_id: String,
    /// Endpoint/container name reported by Docker.
    pub name: String,
    pub endpoint_id: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub mac_address: Option<String>,
}

/// Domain options for creating a Docker network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateNetworkOptions {
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub ipv6: bool,
    pub internal: bool,
    pub attachable: bool,
    pub labels: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
}

impl Default for CreateNetworkOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            driver: "bridge".to_string(),
            subnet: None,
            gateway: None,
            ipv6: false,
            internal: false,
            attachable: false,
            labels: BTreeMap::new(),
            options: BTreeMap::new(),
        }
    }
}

/// Result returned by Docker after creating a network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateNetworkResult {
    pub id: String,
    pub warning: Option<String>,
}
