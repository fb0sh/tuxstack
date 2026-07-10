use anyhow::Result;
use bollard::{container::ListContainersOptions, Docker};
use std::collections::HashMap;
use tuxstack_common::ContainerInfo;

#[derive(Clone)]
pub struct Client {
    docker: Docker,
}

impl Client {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    /// Check if Docker is available
    pub async fn ping(&self) -> Result<bool> {
        self.docker.ping().await?;
        Ok(true)
    }

    /// List all containers (optionally filtered by status)
    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let mut filters = HashMap::new();
        if !all {
            filters.insert("status".to_string(), vec!["running".to_string()]);
        }

        let options = ListContainersOptions {
            all,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;

        // TODO: map bollard types to our ContainerInfo
        Ok(containers
            .into_iter()
            .map(|c| ContainerInfo {
                id: c.id.unwrap_or_default().trim_start_matches("sha256:").to_string(),
                name: c
                    .names
                    .unwrap_or_default()
                    .first()
                    .cloned()
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_string(),
                image: c.image.unwrap_or_default(),
                status: container_status_from_str(
                    &c.state.unwrap_or_default(),
                ),
                created: c.created.unwrap_or_default().to_string(),
                ports: vec![],
                cpu_usage: None,
                memory_usage: None,
                memory_limit: None,
            })
            .collect())
    }
}

/// Convert bollard container state strings to our ContainerStatus
fn container_status_from_str(s: &str) -> tuxstack_common::ContainerStatus {
    match s {
        "running" => tuxstack_common::ContainerStatus::Running,
        "exited" => tuxstack_common::ContainerStatus::Exited,
        "paused" => tuxstack_common::ContainerStatus::Paused,
        "restarting" => tuxstack_common::ContainerStatus::Restarting,
        "removing" => tuxstack_common::ContainerStatus::Removing,
        "dead" => tuxstack_common::ContainerStatus::Dead,
        "created" => tuxstack_common::ContainerStatus::Created,
        other => tuxstack_common::ContainerStatus::Unknown(other.to_string()),
    }
}
