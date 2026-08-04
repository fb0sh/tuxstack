//! Compose label grouping and group lifecycle operations.
//!
//! This service uses Docker Engine metadata only. It does not invoke the
//! `docker compose` CLI and never deletes Compose files, directories, or
//! volumes.

use std::sync::Arc;

use futures_util::{StreamExt, stream};

use crate::client::DockerClient;
use crate::error::DockerError;
use crate::models::{
    ContainerGroupId, ContainerGroupMemberResult, ContainerGroupOperationResult,
    ContainerGroupSummary, RemoveContainerOptions, group_compose_containers,
};

use super::containers::{ContainerService, ListContainersOptions};

const MAX_GROUP_CONCURRENCY: usize = 6;

#[derive(Clone)]
pub struct ComposeService {
    client: Arc<DockerClient>,
    containers: ContainerService,
}

impl ComposeService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self {
            containers: ContainerService::new(client.clone()),
            client,
        }
    }

    /// Build real projects from one `list containers?all=true` response.
    pub async fn list_projects(&self) -> Result<Vec<ContainerGroupSummary>, DockerError> {
        let containers = self
            .containers
            .list_containers(&ListContainersOptions {
                all: true,
                ..Default::default()
            })
            .await?;
        Ok(group_compose_containers(
            &self.client.endpoint_fingerprint(),
            &containers,
        ))
    }

    pub async fn start_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Start).await
    }

    pub async fn stop_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Stop).await
    }

    pub async fn restart_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Restart).await
    }

    pub async fn pause_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Pause).await
    }

    pub async fn unpause_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Unpause).await
    }

    /// Remove members without force and without volumes. Running members fail
    /// individually instead of being stopped or destroyed implicitly.
    pub async fn remove_group(
        &self,
        group_id: &ContainerGroupId,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        self.run_group(group_id, GroupAction::Remove).await
    }

    async fn run_group(
        &self,
        group_id: &ContainerGroupId,
        action: GroupAction,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        if group_id.endpoint_key != self.client.endpoint_fingerprint() {
            return Err(DockerError::InvalidContainerConfig(
                "container group belongs to a different Docker endpoint".into(),
            ));
        }
        let group = self
            .list_projects()
            .await?
            .into_iter()
            .find(|group| group.id == *group_id)
            .ok_or_else(|| {
                DockerError::ContainerNotFound(format!("Compose project {}", group_id.project_name))
            })?;

        let service = self.containers.clone();
        let mut members = stream::iter(group.containers.into_iter().map(move |container_id| {
            let service = service.clone();
            async move {
                let operation = match action {
                    GroupAction::Start => service.start_container(&container_id).await,
                    GroupAction::Stop => service.stop_container(&container_id, None).await,
                    GroupAction::Restart => service.restart_container(&container_id).await,
                    GroupAction::Pause => service.pause_container(&container_id).await,
                    GroupAction::Unpause => service.unpause_container(&container_id).await,
                    GroupAction::Remove => {
                        service
                            .remove_container(
                                &container_id,
                                &RemoveContainerOptions {
                                    force: false,
                                    remove_volumes: false,
                                    remove_links: false,
                                },
                            )
                            .await
                    }
                };
                ContainerGroupMemberResult {
                    container_id,
                    error: operation.err().map(|error| error.to_string()),
                }
            }
        }))
        .buffer_unordered(MAX_GROUP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        members.sort_by(|left, right| left.container_id.cmp(&right.container_id));

        Ok(ContainerGroupOperationResult {
            group_id: group_id.clone(),
            members,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum GroupAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}
