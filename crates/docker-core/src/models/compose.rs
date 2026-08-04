//! Compose metadata and label-only grouping.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ContainerRuntimeState, ContainerSummary};

pub const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
pub const COMPOSE_SERVICE_LABEL: &str = "com.docker.compose.service";
pub const COMPOSE_CONTAINER_NUMBER_LABEL: &str = "com.docker.compose.container-number";
pub const COMPOSE_WORKING_DIR_LABEL: &str = "com.docker.compose.project.working_dir";
pub const COMPOSE_CONFIG_FILES_LABEL: &str = "com.docker.compose.project.config_files";
pub const COMPOSE_VERSION_LABEL: &str = "com.docker.compose.version";
pub const COMPOSE_ONEOFF_LABEL: &str = "com.docker.compose.oneoff";
pub const DEVCONTAINER_LOCAL_FOLDER_LABEL: &str = "devcontainer.local_folder";
pub const DEVCONTAINER_CONFIG_FILE_LABEL: &str = "devcontainer.config_file";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeContainerMetadata {
    pub project_name: String,
    pub service: String,
    pub container_number: Option<u32>,
    pub working_directory: Option<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub compose_version: Option<String>,
    pub oneoff: bool,
    pub devcontainer_local_folder: Option<PathBuf>,
    pub devcontainer_config_file: Option<PathBuf>,
}

impl ComposeContainerMetadata {
    /// Parse Compose metadata exclusively from labels. A project label is
    /// mandatory; container names are deliberately never inspected.
    pub fn from_labels(labels: &BTreeMap<String, String>) -> Option<Self> {
        let project_name = nonempty(labels.get(COMPOSE_PROJECT_LABEL))?.to_string();
        Some(Self {
            project_name,
            service: labels
                .get(COMPOSE_SERVICE_LABEL)
                .map(|value| value.trim().to_string())
                .unwrap_or_default(),
            container_number: labels
                .get(COMPOSE_CONTAINER_NUMBER_LABEL)
                .and_then(|value| value.parse().ok()),
            working_directory: labels
                .get(COMPOSE_WORKING_DIR_LABEL)
                .and_then(|value| nonempty(Some(value)))
                .map(PathBuf::from),
            config_files: labels
                .get(COMPOSE_CONFIG_FILES_LABEL)
                .map(|value| split_config_files(value))
                .unwrap_or_default(),
            compose_version: labels
                .get(COMPOSE_VERSION_LABEL)
                .and_then(|value| nonempty(Some(value)))
                .map(str::to_string),
            oneoff: labels.get(COMPOSE_ONEOFF_LABEL).is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes")
            }),
            devcontainer_local_folder: labels
                .get(DEVCONTAINER_LOCAL_FOLDER_LABEL)
                .and_then(|value| nonempty(Some(value)))
                .map(PathBuf::from),
            devcontainer_config_file: labels
                .get(DEVCONTAINER_CONFIG_FILE_LABEL)
                .and_then(|value| nonempty(Some(value)))
                .map(PathBuf::from),
        })
    }

    pub fn is_devcontainer(&self) -> bool {
        self.devcontainer_local_folder.is_some() || self.devcontainer_config_file.is_some()
    }
}

fn nonempty(value: Option<&String>) -> Option<&str> {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn split_config_files(value: &str) -> Vec<PathBuf> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContainerGroupId {
    pub endpoint_key: String,
    pub project_name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupOperationState {
    #[default]
    Idle,
    Starting,
    Stopping,
    Restarting,
    Pausing,
    Unpausing,
    Removing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerGroupSection {
    Restarting,
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerGroupSummary {
    pub id: ContainerGroupId,
    pub project_name: String,
    pub display_name: String,
    pub containers: Vec<String>,
    pub total_count: usize,
    pub running_count: usize,
    pub paused_count: usize,
    pub restarting_count: usize,
    pub stopped_count: usize,
    pub unhealthy_count: usize,
    pub oneoff_count: usize,
    pub devcontainer_count: usize,
    pub working_directory: Option<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub compose_version: Option<String>,
    pub operation_state: GroupOperationState,
}

impl ContainerGroupSummary {
    /// Restarting has priority, followed by running, paused and stopped.
    pub fn section(&self) -> ContainerGroupSection {
        if self.restarting_count > 0 {
            ContainerGroupSection::Restarting
        } else if self.running_count > 0 {
            ContainerGroupSection::Running
        } else if self.paused_count > 0 {
            ContainerGroupSection::Paused
        } else {
            ContainerGroupSection::Stopped
        }
    }
}

/// Compatibility name for the former planned Compose project model.
pub type ComposeProject = ContainerGroupSummary;

/// The result for one member of a group lifecycle request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerGroupMemberResult {
    pub container_id: String,
    pub error: Option<String>,
}

impl ContainerGroupMemberResult {
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

/// A group request always reports every attempted member. A failed member does
/// not hide successful operations on its siblings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerGroupOperationResult {
    pub group_id: ContainerGroupId,
    pub members: Vec<ContainerGroupMemberResult>,
}

impl ContainerGroupOperationResult {
    pub fn is_complete_success(&self) -> bool {
        self.members
            .iter()
            .all(ContainerGroupMemberResult::succeeded)
    }

    pub fn failure_count(&self) -> usize {
        self.members
            .iter()
            .filter(|member| !member.succeeded())
            .count()
    }
}

/// Build groups in stable project-name order. Containers without the Compose
/// project label are omitted and remain individual list entries.
pub fn group_compose_containers(
    endpoint_key: &str,
    containers: &[ContainerSummary],
) -> Vec<ContainerGroupSummary> {
    let mut projects: BTreeMap<String, Vec<(&ContainerSummary, ComposeContainerMetadata)>> =
        BTreeMap::new();
    for container in containers {
        if let Some(metadata) = container.compose_metadata() {
            projects
                .entry(metadata.project_name.clone())
                .or_default()
                .push((container, metadata));
        }
    }

    projects
        .into_iter()
        .map(|(project_name, mut members)| {
            members.sort_by(|(a, a_meta), (b, b_meta)| {
                a_meta
                    .service
                    .to_ascii_lowercase()
                    .cmp(&b_meta.service.to_ascii_lowercase())
                    .then_with(|| a_meta.container_number.cmp(&b_meta.container_number))
                    .then_with(|| {
                        a.name
                            .to_ascii_lowercase()
                            .cmp(&b.name.to_ascii_lowercase())
                    })
                    .then_with(|| a.id.cmp(&b.id))
            });
            let running_count = members
                .iter()
                .filter(|(container, _)| container.state == ContainerRuntimeState::Running)
                .count();
            let paused_count = members
                .iter()
                .filter(|(container, _)| container.state == ContainerRuntimeState::Paused)
                .count();
            let restarting_count = members
                .iter()
                .filter(|(container, _)| container.state == ContainerRuntimeState::Restarting)
                .count();
            let stopped_count = members.len() - running_count - paused_count - restarting_count;
            let unhealthy_count = members
                .iter()
                .filter(|(container, _)| {
                    container.health_summary().is_some_and(|health| {
                        health.status == super::ContainerHealthState::Unhealthy
                    })
                })
                .count();
            let oneoff_count = members
                .iter()
                .filter(|(_, metadata)| metadata.oneoff)
                .count();
            let devcontainer_count = members
                .iter()
                .filter(|(_, metadata)| metadata.is_devcontainer())
                .count();
            let working_directory = most_common(
                members
                    .iter()
                    .filter_map(|(_, metadata)| metadata.working_directory.clone()),
            );
            let compose_version = most_common(
                members
                    .iter()
                    .filter_map(|(_, metadata)| metadata.compose_version.clone()),
            );
            let mut config_files = members
                .iter()
                .flat_map(|(_, metadata)| metadata.config_files.clone())
                .collect::<Vec<_>>();
            config_files.sort();
            config_files.dedup();
            let container_ids = members
                .iter()
                .map(|(container, _)| container.id.clone())
                .collect::<Vec<_>>();

            ContainerGroupSummary {
                id: ContainerGroupId {
                    endpoint_key: endpoint_key.to_string(),
                    project_name: project_name.clone(),
                },
                display_name: project_name.clone(),
                project_name,
                total_count: members.len(),
                containers: container_ids,
                running_count,
                paused_count,
                restarting_count,
                stopped_count,
                unhealthy_count,
                oneoff_count,
                devcontainer_count,
                working_directory,
                config_files,
                compose_version,
                operation_state: GroupOperationState::Idle,
            }
        })
        .collect()
}

fn most_common<T>(values: impl Iterator<Item = T>) -> Option<T>
where
    T: Clone + Eq + std::hash::Hash + Ord,
{
    let mut counts = HashMap::<T, usize>::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|(a_value, a_count), (b_value, b_count)| {
            a_count.cmp(b_count).then_with(|| b_value.cmp(a_value))
        })
        .map(|(value, _)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn container(
        name: &str,
        state: ContainerRuntimeState,
        labels: &[(&str, &str)],
    ) -> ContainerSummary {
        ContainerSummary {
            id: format!("id-{name}"),
            short_id: format!("short-{name}"),
            name: name.into(),
            image: "busybox".into(),
            image_id: "image".into(),
            state,
            status: if name == "bad" {
                "Up (unhealthy)"
            } else {
                state.as_str()
            }
            .into(),
            created_at: Utc.timestamp_opt(1, 0).unwrap(),
            ports: vec![],
            labels: labels
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
        }
    }

    #[test]
    fn grouping_requires_project_label_and_never_guesses_name() {
        let guessed = container("project_service_1", ContainerRuntimeState::Running, &[]);
        assert!(group_compose_containers("local", &[guessed]).is_empty());
    }

    #[test]
    fn parses_oneoff_devcontainer_and_group_counts() {
        let labels = [
            (COMPOSE_PROJECT_LABEL, "demo"),
            (COMPOSE_SERVICE_LABEL, "web"),
            (COMPOSE_CONTAINER_NUMBER_LABEL, "2"),
            (COMPOSE_WORKING_DIR_LABEL, "/work/demo"),
            (
                COMPOSE_CONFIG_FILES_LABEL,
                "/work/demo/compose.yml,/work/demo/dev.yml",
            ),
            (COMPOSE_VERSION_LABEL, "2.29.0"),
            (COMPOSE_ONEOFF_LABEL, "True"),
            (DEVCONTAINER_LOCAL_FOLDER_LABEL, "/work/demo"),
            (
                DEVCONTAINER_CONFIG_FILE_LABEL,
                "/work/demo/.devcontainer/devcontainer.json",
            ),
        ];
        let groups = group_compose_containers(
            "unix:///docker.sock",
            &[
                container("web", ContainerRuntimeState::Running, &labels),
                container("bad", ContainerRuntimeState::Paused, &labels),
                container("restart", ContainerRuntimeState::Restarting, &labels),
                container("db", ContainerRuntimeState::Exited, &labels),
            ],
        );
        let group = &groups[0];
        assert_eq!(group.total_count, 4);
        assert_eq!(group.running_count, 1);
        assert_eq!(group.paused_count, 1);
        assert_eq!(group.restarting_count, 1);
        assert_eq!(group.stopped_count, 1);
        assert_eq!(group.unhealthy_count, 1);
        assert_eq!(group.oneoff_count, 4);
        assert_eq!(group.devcontainer_count, 4);
        assert_eq!(group.section(), ContainerGroupSection::Restarting);
        assert_eq!(group.config_files.len(), 2);
    }

    #[test]
    fn groups_and_members_have_stable_sorting() {
        let project = |name: &'static str, service: &'static str| {
            [
                (COMPOSE_PROJECT_LABEL, name),
                (COMPOSE_SERVICE_LABEL, service),
            ]
        };
        let groups = group_compose_containers(
            "endpoint",
            &[
                container("z", ContainerRuntimeState::Exited, &project("zeta", "z")),
                container("b", ContainerRuntimeState::Exited, &project("alpha", "b")),
                container("a", ContainerRuntimeState::Exited, &project("alpha", "a")),
            ],
        );
        assert_eq!(
            groups
                .iter()
                .map(|group| group.project_name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert_eq!(groups[0].containers, vec!["id-a", "id-b"]);
    }
}
