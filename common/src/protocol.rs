use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// Method names for the tuxstack daemon API
pub mod methods {
    // Docker
    pub const DOCKER_LIST_CONTAINERS: &str = "docker.list_containers";
    pub const DOCKER_CONTAINER_ACTION: &str = "docker.container_action";
    pub const DOCKER_CONTAINER_LOGS: &str = "docker.container_logs";
    pub const DOCKER_LIST_IMAGES: &str = "docker.list_images";
    pub const DOCKER_PULL_IMAGE: &str = "docker.pull_image";
    pub const DOCKER_REMOVE_IMAGE: &str = "docker.remove_image";
    pub const DOCKER_LIST_COMPOSE: &str = "docker.list_compose";
    pub const DOCKER_COMPOSE_ACTION: &str = "docker.compose_action";
    pub const DOCKER_CONTAINER_STATS: &str = "docker.container_stats";

    // Incus
    pub const INCUS_LIST_INSTANCES: &str = "incus.list_instances";
    pub const INCUS_INSTANCE_ACTION: &str = "incus.instance_action";
    pub const INCUS_CREATE_INSTANCE: &str = "incus.create_instance";
    pub const INCUS_INSTANCE_TERMINAL: &str = "incus.instance_terminal";

    // System
    pub const SYSTEM_STATUS: &str = "system.status";
    pub const SYSTEM_DETECT: &str = "system.detect";
    pub const SYSTEM_INSTALL_DOCKER: &str = "system.install_docker";
    pub const SYSTEM_INSTALL_INCUS: &str = "system.install_incus";
}
