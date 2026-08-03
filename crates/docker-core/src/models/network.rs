//! Network domain models.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A short list entry for a network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkSummary {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: bool,
    pub attachable: bool,
    pub ingress: bool,
    pub ipv6: bool,
    pub labels: BTreeMap<String, String>,
}

/// Detailed view of a network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkDetail {
    pub summary: NetworkSummary,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub containers: Vec<NetworkContainer>,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkContainer {
    pub name: String,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub mac: Option<String>,
}
