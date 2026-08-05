//! Compose label grouping and group lifecycle operations.
//!
//! This service uses Docker Engine metadata only. It does not invoke the
//! `docker compose` CLI and never deletes Compose files, directories, or
//! volumes.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use futures_util::{StreamExt, stream};

use crate::client::DockerClient;
use crate::error::DockerError;
use crate::models::{
    ContainerGroupId, ContainerGroupMemberResult, ContainerGroupOperationResult,
    ContainerGroupSummary, KillContainerOptions, RemoveContainerOptions, RestartContainerOptions,
    StopContainerOptions, group_compose_containers,
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

    /// Execute one lifecycle action against exactly `target_ids`.
    ///
    /// The project is re-listed only to validate that each requested target is
    /// still a member. Other project members are never acted on. A stale or
    /// foreign target is retained as a typed member failure, alongside results
    /// for valid targets.
    pub async fn execute_group_targets(
        &self,
        group_id: &ContainerGroupId,
        target_ids: &[String],
        action: ComposeGroupAction,
    ) -> Result<ContainerGroupOperationResult, DockerError> {
        if group_id.endpoint_key != self.client.endpoint_fingerprint() {
            return Err(DockerError::InvalidContainerConfig(
                "container group belongs to a different Docker endpoint".into(),
            ));
        }
        let project_members = self
            .list_projects()
            .await?
            .into_iter()
            .find(|group| group.id == *group_id)
            .ok_or_else(|| {
                DockerError::ContainerNotFound(format!("Compose project {}", group_id.project_name))
            })?
            .containers
            .into_iter()
            .collect::<HashSet<_>>();
        let service = self.containers.clone();

        Ok(execute_explicit_targets(
            group_id,
            target_ids,
            &project_members,
            move |container_id| {
                let service = service.clone();
                let action = action.clone();
                async move { execute_action(&service, &container_id, &action).await }
            },
        )
        .await)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeGroupAction {
    Start,
    Stop(StopContainerOptions),
    Restart(RestartContainerOptions),
    Kill(KillContainerOptions),
    Pause,
    Unpause,
    Remove(RemoveContainerOptions),
}

async fn execute_action(
    service: &ContainerService,
    container_id: &str,
    action: &ComposeGroupAction,
) -> Result<(), DockerError> {
    match action {
        ComposeGroupAction::Start => service.start_container(container_id).await,
        ComposeGroupAction::Stop(options) => {
            service.stop_container(container_id, Some(options)).await
        }
        ComposeGroupAction::Restart(options) => {
            service
                .restart_container_with_options(container_id, options)
                .await
        }
        ComposeGroupAction::Kill(options) => {
            service
                .kill_container_with_options(container_id, options)
                .await
        }
        ComposeGroupAction::Pause => service.pause_container(container_id).await,
        ComposeGroupAction::Unpause => service.unpause_container(container_id).await,
        ComposeGroupAction::Remove(options) => {
            service.remove_container(container_id, options).await
        }
    }
}

async fn execute_explicit_targets<F, Fut>(
    group_id: &ContainerGroupId,
    target_ids: &[String],
    project_members: &HashSet<String>,
    execute: F,
) -> ContainerGroupOperationResult
where
    F: Fn(String) -> Fut + Clone,
    Fut: Future<Output = Result<(), DockerError>>,
{
    let project_name = group_id.project_name.clone();
    let mut members = stream::iter(target_ids.iter().cloned().map(|container_id| {
        let belongs_to_project = project_members.contains(&container_id);
        let project_name = project_name.clone();
        let execute = execute.clone();
        async move {
            let operation = if belongs_to_project {
                execute(container_id.clone()).await
            } else {
                Err(DockerError::InvalidContainerConfig(format!(
                    "container {container_id} does not belong to Compose project {project_name}"
                )))
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

    ContainerGroupOperationResult {
        group_id: group_id.clone(),
        members,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn explicit_targets_execute_only_requested_members_and_keep_partial_failures() {
        let group_id = ContainerGroupId {
            endpoint_key: "local".into(),
            project_name: "demo".into(),
        };
        let project_members = HashSet::from([
            "running".to_string(),
            "stopped".to_string(),
            "failed".to_string(),
        ]);
        // This models a filtered start operation: the running member is part
        // of the project but absent from the controller's target list.
        let targets = vec![
            "stopped".to_string(),
            "failed".to_string(),
            "foreign".to_string(),
        ];
        let executed = Arc::new(Mutex::new(Vec::new()));
        let result = execute_explicit_targets(&group_id, &targets, &project_members, {
            let executed = executed.clone();
            move |container_id| {
                let executed = executed.clone();
                async move {
                    executed.lock().unwrap().push(container_id.clone());
                    if container_id == "failed" {
                        Err(DockerError::Conflict("daemon rejected target".into()))
                    } else {
                        Ok(())
                    }
                }
            }
        })
        .await;

        let mut executed = executed.lock().unwrap().clone();
        executed.sort();
        assert_eq!(executed, ["failed", "stopped"]);
        assert_eq!(
            result
                .members
                .iter()
                .map(|member| member.container_id.as_str())
                .collect::<Vec<_>>(),
            ["failed", "foreign", "stopped"]
        );
        assert_eq!(result.failure_count(), 2);
        assert!(
            result.members[0]
                .error
                .as_deref()
                .unwrap()
                .contains("daemon rejected target")
        );
        assert!(
            result.members[1]
                .error
                .as_deref()
                .unwrap()
                .contains("does not belong to Compose project demo")
        );
        assert!(result.members[2].succeeded());
    }
}
