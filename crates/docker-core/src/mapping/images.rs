//! Mapping and association for image DTOs.

use std::collections::{BTreeMap, HashMap, HashSet};

use bollard::models::{ImageInspect, ImageSummary as BollardImageSummary};
use chrono::{TimeZone, Utc};

use crate::mapping::containers::short_id;
use crate::models::{
    ContainerSummary, EnvironmentVariable, ImageContainerReference, ImageDetail, ImageSummary,
};

/// Normalize an image ID for comparisons while retaining only hexadecimal
/// digest data. Docker may return `sha256:`, `sha256-`, or a bare full/short ID.
pub fn normalize_image_id(id: &str) -> String {
    let trimmed = id.trim();
    trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("sha256-"))
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

/// Return Docker's canonical full-ID representation while still accepting
/// short IDs used by old daemons and tests.
pub fn canonical_image_id(id: &str) -> String {
    let normalized = normalize_image_id(id);
    if normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        format!("sha256:{normalized}")
    } else {
        normalized
    }
}

fn valid_references(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty() && value != "<none>:<none>" && value != "<none>@<none>")
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

pub fn display_name(repo_tags: &[String]) -> String {
    repo_tags
        .first()
        .cloned()
        .unwrap_or_else(|| "<none>:<none>".to_string())
}

fn positive(value: i64) -> Option<u64> {
    (value >= 0).then_some(value as u64)
}

fn refs_by_image_id(
    containers: &[ContainerSummary],
) -> HashMap<String, Vec<ImageContainerReference>> {
    let mut refs: HashMap<String, Vec<ImageContainerReference>> = HashMap::new();
    for container in containers {
        let image_id = normalize_image_id(&container.image_id);
        if image_id.is_empty() {
            continue;
        }
        refs.entry(image_id)
            .or_default()
            .push(ImageContainerReference {
                id: container.id.clone(),
                short_id: container.short_id.clone(),
                name: container.name.clone(),
                state: container.state,
                status: container.status.clone(),
                created_at: Some(container.created_at),
            });
    }
    for containers in refs.values_mut() {
        containers.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        containers.dedup_by(|a, b| a.id == b.id);
    }
    refs
}

/// Map, normalize, deduplicate and associate image list entries with *all*
/// existing containers. Duplicate IDs are merged so tags never inflate image
/// counts or total logical size.
pub(crate) fn map_image_summaries(
    images: Vec<BollardImageSummary>,
    containers: &[ContainerSummary],
) -> Vec<ImageSummary> {
    let mut unique: HashMap<String, ImageSummary> = HashMap::new();

    for image in images {
        let canonical_key = normalize_image_id(&image.id);
        let repo_tags = valid_references(image.repo_tags);
        let repo_digests = valid_references(image.repo_digests);
        let labels: BTreeMap<_, _> = image.labels.into_iter().collect();
        let created_at = Utc.timestamp_opt(image.created, 0).single();
        let size_bytes = positive(image.size).unwrap_or(0);
        let shared_size_bytes = positive(image.shared_size);

        unique
            .entry(canonical_key)
            .and_modify(|current| {
                for tag in &repo_tags {
                    if !current.repo_tags.contains(tag) {
                        current.repo_tags.push(tag.clone());
                    }
                }
                for digest in &repo_digests {
                    if !current.repo_digests.contains(digest) {
                        current.repo_digests.push(digest.clone());
                    }
                }
                current.labels.extend(labels.clone());
                current.size_bytes = current.size_bytes.max(size_bytes);
                current.shared_size_bytes = current.shared_size_bytes.or(shared_size_bytes);
                current.created_at = current.created_at.or(created_at);
                current.display_name = display_name(&current.repo_tags);
            })
            .or_insert_with(|| ImageSummary {
                short_id: short_id(&image.id),
                id: canonical_image_id(&image.id),
                display_name: display_name(&repo_tags),
                repo_tags,
                repo_digests,
                created_at,
                size_bytes,
                shared_size_bytes,
                // The v1.53 list/inspect schema no longer provides VirtualSize.
                virtual_size_bytes: None,
                labels,
                containers: Vec::new(),
                in_use: false,
            });
    }

    let mut refs = refs_by_image_id(containers);
    let mut mapped: Vec<_> = unique.into_values().collect();
    for image in &mut mapped {
        image.containers = refs
            .remove(&normalize_image_id(&image.id))
            .unwrap_or_default();
        image.in_use = !image.containers.is_empty();
    }
    mapped
}

/// Map a single list entry without container associations.
#[cfg(test)]
pub(crate) fn map_image_summary(image: BollardImageSummary) -> ImageSummary {
    map_image_summaries(vec![image], &[])
        .into_iter()
        .next()
        .expect("one input image produces one mapped image")
}

/// Map a typed image inspect response. `containers` must be the complete
/// container list so stopped/created containers remain associated.
pub(crate) fn map_image_detail(
    inspect: ImageInspect,
    containers: &[ContainerSummary],
) -> ImageDetail {
    let config = inspect.config.clone().unwrap_or_default();
    let labels: BTreeMap<_, _> = config
        .labels
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let id = canonical_image_id(&inspect.id.clone().unwrap_or_default());
    let repo_tags = valid_references(inspect.repo_tags.clone().unwrap_or_default());
    let repo_digests = valid_references(inspect.repo_digests.clone().unwrap_or_default());
    let created_at = inspect.created;
    let size_bytes = inspect.size.and_then(positive).unwrap_or(0);

    let mut summary = ImageSummary {
        short_id: short_id(&id),
        id,
        display_name: display_name(&repo_tags),
        repo_tags,
        repo_digests,
        created_at,
        size_bytes,
        shared_size_bytes: None,
        virtual_size_bytes: None,
        labels: labels.clone(),
        containers: Vec::new(),
        in_use: false,
    };
    summary.containers = refs_by_image_id(containers)
        .remove(&normalize_image_id(&summary.id))
        .unwrap_or_default();
    summary.in_use = !summary.containers.is_empty();

    ImageDetail {
        summary,
        architecture: inspect.architecture,
        os: inspect.os,
        variant: inspect.variant,
        author: inspect.author,
        // Current Docker ImageInspect does not expose this legacy field.
        docker_version: None,
        comment: inspect.comment,
        command: config.cmd.unwrap_or_default(),
        entrypoint: config.entrypoint.unwrap_or_default(),
        environment: config
            .env
            .unwrap_or_default()
            .into_iter()
            .map(|line| match line.split_once('=') {
                Some((name, value)) => EnvironmentVariable {
                    name: name.to_string(),
                    value: Some(value.to_string()),
                },
                None => EnvironmentVariable {
                    name: line,
                    value: None,
                },
            })
            .collect(),
        working_dir: config.working_dir,
        user: config.user,
        stop_signal: config.stop_signal,
        shell: config.shell.unwrap_or_default(),
        labels,
        root_fs_layers: inspect
            .root_fs
            .and_then(|root_fs| root_fs.layers)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use bollard::models::{ImageConfig, ImageInspectRootFs};
    use chrono::TimeZone;

    use super::*;
    use crate::models::ContainerState;

    fn sample(id: &str) -> BollardImageSummary {
        BollardImageSummary {
            id: id.into(),
            repo_tags: vec!["nginx:latest".into(), "nginx:1.27".into()],
            repo_digests: vec![
                "nginx@sha256:def".into(),
                "nginx@sha256:fed".into(),
                "nginx@sha256:def".into(),
            ],
            created: 1_700_000_000,
            size: 100_000_000,
            shared_size: 20_000_000,
            labels: HashMap::from([("org.example.version".into(), "1".into())]),
            ..Default::default()
        }
    }

    fn container(image_id: &str, name: &str) -> ContainerSummary {
        container_with_state(image_id, name, ContainerState::Exited)
    }

    fn container_with_state(image_id: &str, name: &str, state: ContainerState) -> ContainerSummary {
        ContainerSummary {
            id: format!("{name}012345678901234567890"),
            short_id: format!("{name}012345"),
            name: name.into(),
            image: "nginx:latest".into(),
            image_id: image_id.into(),
            state,
            status: state.as_str().into(),
            created_at: Utc.timestamp_opt(1_700_000_010, 0).unwrap(),
            ports: vec![],
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn maps_summary_tags_digests_sizes_and_timestamp() {
        let mapped = map_image_summary(sample(
            "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        ));
        assert_eq!(mapped.repo_tags, vec!["nginx:latest", "nginx:1.27"]);
        assert_eq!(
            mapped.repo_digests,
            vec!["nginx@sha256:def", "nginx@sha256:fed"]
        );
        assert_eq!(mapped.created_at.unwrap().timestamp(), 1_700_000_000);
        assert_eq!(mapped.size_bytes, 100_000_000);
        assert_eq!(mapped.shared_size_bytes, Some(20_000_000));
        assert_eq!(mapped.short_id, "abcdef123456");
    }

    #[test]
    fn dangling_image_has_stable_display_name() {
        let mut image = sample("sha256:abcdef123456");
        image.repo_tags = vec!["<none>:<none>".into()];
        let mapped = map_image_summary(image);
        assert!(mapped.repo_tags.is_empty());
        assert_eq!(mapped.display_name, "<none>:<none>");
        assert_eq!(mapped.primary_tag(), "<none>:<none>");
    }

    #[test]
    fn normalizes_all_supported_id_forms() {
        assert_eq!(normalize_image_id("sha256:ABCDEF"), "abcdef");
        assert_eq!(normalize_image_id("sha256-ABCDEF"), "abcdef");
        assert_eq!(normalize_image_id("ABCDEF"), "abcdef");
        assert_eq!(canonical_image_id("ABCDEF"), "abcdef");
        assert_eq!(
            canonical_image_id(&"A".repeat(64)),
            format!("sha256:{}", "a".repeat(64))
        );
    }

    #[test]
    fn deduplicates_same_image_and_merges_tags() {
        let mut second = sample("abcdef1234567890");
        second.repo_tags = vec!["example:other".into()];
        let mapped = map_image_summaries(vec![sample("sha256:abcdef1234567890"), second], &[]);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].size_bytes, 100_000_000);
        assert_eq!(mapped[0].repo_tags.len(), 3);
    }

    #[test]
    fn associates_containers_by_image_id_only() {
        let image = sample("sha256:abcdef1234567890abcdef");
        let containers = vec![
            container("sha256:abcdef1234567890abcdef", "full"),
            container("abcdef123456", "short"),
            container("", "tag-only"),
        ];
        let mapped = map_image_summaries(vec![image], &containers);
        assert!(mapped[0].in_use);
        assert_eq!(mapped[0].containers.len(), 1);
        assert_eq!(mapped[0].containers[0].name, "full");
    }

    #[test]
    fn every_existing_container_state_marks_matching_image_in_use() {
        let id = "sha256:abcdef1234567890abcdef";
        let states = [
            ContainerState::Running,
            ContainerState::Created,
            ContainerState::Paused,
            ContainerState::Exited,
            ContainerState::Dead,
        ];
        let containers: Vec<_> = states
            .into_iter()
            .enumerate()
            .map(|(index, state)| container_with_state(id, &format!("state-{index}"), state))
            .collect();
        let mapped = map_image_summaries(vec![sample(id)], &containers);
        assert!(mapped[0].in_use);
        assert_eq!(mapped[0].containers.len(), states.len());
    }

    #[test]
    fn matching_tag_without_image_id_does_not_mark_image_in_use() {
        let mapped = map_image_summaries(
            vec![sample("sha256:abcdef1234567890abcdef")],
            &[container("", "tag-only")],
        );
        assert!(!mapped[0].in_use);
        assert!(mapped[0].containers.is_empty());
    }

    #[test]
    fn image_without_container_is_unused() {
        let mapped = map_image_summaries(vec![sample("sha256:abcdef123456")], &[]);
        assert!(!mapped[0].in_use);
        assert!(mapped[0].containers.is_empty());
    }

    #[test]
    fn inspect_maps_platform_config_environment_labels_and_layers() {
        let inspect = ImageInspect {
            id: Some("sha256:abcdef1234567890".into()),
            repo_tags: Some(vec!["example:test".into()]),
            repo_digests: Some(vec!["example@sha256:123".into()]),
            created: Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
            architecture: Some("arm64".into()),
            variant: Some("v8".into()),
            os: Some("linux".into()),
            author: Some("author".into()),
            comment: Some("comment".into()),
            size: Some(42),
            config: Some(ImageConfig {
                cmd: Some(vec!["echo".into(), "hello world".into()]),
                entrypoint: Some(vec!["/bin/sh".into(), "-c".into()]),
                env: Some(vec!["EMPTY=".into(), "TOKEN=a=b".into(), "UNSET".into()]),
                labels: Some(HashMap::from([("org.example".into(), "yes".into())])),
                working_dir: Some("/work".into()),
                user: Some("1000".into()),
                stop_signal: Some("SIGTERM".into()),
                shell: Some(vec!["/bin/sh".into(), "-c".into()]),
                ..Default::default()
            }),
            root_fs: Some(ImageInspectRootFs {
                typ: "layers".into(),
                layers: Some(vec!["sha256:layer".into()]),
            }),
            ..Default::default()
        };
        let detail = map_image_detail(inspect, &[]);
        assert_eq!(
            detail.summary.created_at.unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(detail.architecture.as_deref(), Some("arm64"));
        assert_eq!(detail.variant.as_deref(), Some("v8"));
        assert_eq!(detail.os.as_deref(), Some("linux"));
        assert_eq!(detail.author.as_deref(), Some("author"));
        assert_eq!(detail.comment.as_deref(), Some("comment"));
        assert_eq!(detail.command, vec!["echo", "hello world"]);
        assert_eq!(detail.entrypoint, vec!["/bin/sh", "-c"]);
        assert_eq!(detail.working_dir.as_deref(), Some("/work"));
        assert_eq!(detail.user.as_deref(), Some("1000"));
        assert_eq!(detail.stop_signal.as_deref(), Some("SIGTERM"));
        assert_eq!(detail.shell, vec!["/bin/sh", "-c"]);
        assert_eq!(detail.environment[0].display(), "EMPTY=");
        assert_eq!(detail.environment[1].display(), "TOKEN=a=b");
        assert_eq!(detail.environment[2].display(), "UNSET");
        assert_eq!(
            detail.labels.get("org.example").map(String::as_str),
            Some("yes")
        );
        assert_eq!(detail.root_fs_layers, vec!["sha256:layer"]);
    }

    #[test]
    fn negative_and_unknown_sizes_are_safe() {
        let mut image = sample("sha256:abcdef123456");
        image.size = -1;
        image.shared_size = -1;
        let mapped = map_image_summary(image);
        assert_eq!(mapped.size_bytes, 0);
        assert_eq!(mapped.shared_size_bytes, None);
    }
}
