//! Pure image view models used by the Qt bridge.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use tuxstack_domain::{ImageContainerReference, ImageDetail, ImageSummary};

/// One row in the image list. This type deliberately contains no Qt values.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRow {
    pub image_id: String,
    pub short_id: String,
    pub display_name: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub secondary_text: String,
    pub size_bytes: u64,
    pub size_text: String,
    pub created_at: Option<DateTime<Utc>>,
    pub created_text: String,
    pub architecture: String,
    pub in_use: bool,
    pub used_by_count: usize,
    pub containers: Vec<ImageContainerReference>,
    pub labels: BTreeMap<String, String>,
}

impl From<&ImageSummary> for ImageRow {
    fn from(summary: &ImageSummary) -> Self {
        let size_text = format_bytes(summary.size_bytes);
        let created_text = summary
            .created_at
            .map(relative_time)
            .unwrap_or_else(|| "Unknown creation time".to_string());
        Self {
            image_id: summary.id.clone(),
            short_id: summary.short_id.clone(),
            display_name: summary.display_name.clone(),
            repo_tags: summary.repo_tags.clone(),
            repo_digests: summary.repo_digests.clone(),
            secondary_text: format!("{size_text} · {created_text}"),
            size_bytes: summary.size_bytes,
            size_text,
            created_at: summary.created_at,
            created_text,
            // Summary listing intentionally does not trigger N inspect calls.
            architecture: "unknown".to_string(),
            in_use: summary.in_use,
            used_by_count: summary.containers.len(),
            containers: summary.containers.clone(),
            labels: summary.labels.clone(),
        }
    }
}

/// A key/value row exposed to QML as a QVariantMap inside a QVariantList.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueRow {
    pub key: String,
    pub value: String,
}

/// A container usage row exposed to QML as structured values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    pub container_id: String,
    pub short_id: String,
    pub name: String,
    pub state: String,
    pub status: String,
    pub created_at: String,
}

/// Scalar and structured detail values. No JSON crosses the Qt boundary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageDetailView {
    pub image_id: String,
    pub short_id: String,
    pub display_name: String,
    pub repo_tags: Vec<String>,
    pub tags_text: String,
    pub digests_text: String,
    pub created_text: String,
    pub created_full_text: String,
    pub size_text: String,
    pub virtual_size_text: String,
    pub platform: String,
    pub architecture: String,
    pub os: String,
    pub author: String,
    pub docker_version: String,
    pub comment: String,
    pub command: String,
    pub entrypoint: String,
    pub working_dir: String,
    pub user: String,
    pub stop_signal: String,
    pub shell: String,
    pub environment: Vec<KeyValueRow>,
    pub labels: Vec<KeyValueRow>,
    pub usage: Vec<UsageRow>,
}

impl From<&ImageDetail> for ImageDetailView {
    fn from(detail: &ImageDetail) -> Self {
        let summary = &detail.summary;
        let architecture_value = non_empty(detail.architecture.as_deref());
        let os_value = non_empty(detail.os.as_deref());
        let variant_value = non_empty(detail.variant.as_deref());
        let architecture = value_or_dash(architecture_value);
        let os = value_or_dash(os_value);
        let platform = match (os_value, architecture_value, variant_value) {
            (Some(os), Some(architecture), Some(variant)) => {
                format!("{os}/{architecture}/{variant}")
            }
            (Some(os), Some(architecture), None) => format!("{os}/{architecture}"),
            _ => "—".to_string(),
        };
        let mut environment: Vec<_> = detail
            .environment
            .iter()
            .map(|item| KeyValueRow {
                key: item.name.clone(),
                value: item.value.clone().unwrap_or_default(),
            })
            .collect();
        environment.sort_by_key(|item| item.key.to_lowercase());
        let labels = sorted_pairs(&detail.labels);
        let usage = summary
            .containers
            .iter()
            .map(|container| UsageRow {
                container_id: container.id.clone(),
                short_id: container.short_id.clone(),
                name: container.name.clone(),
                state: container.state.as_str().to_string(),
                status: container.status.clone(),
                created_at: container
                    .created_at
                    .map(|created| created.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default(),
            })
            .collect();

        Self {
            image_id: summary.id.clone(),
            short_id: summary.short_id.clone(),
            display_name: summary.display_name.clone(),
            repo_tags: summary.repo_tags.clone(),
            tags_text: join_or_dash(&summary.repo_tags),
            digests_text: join_or_dash(&summary.repo_digests),
            created_text: summary
                .created_at
                .map(relative_time)
                .unwrap_or_else(|| "—".to_string()),
            created_full_text: summary
                .created_at
                .map(full_utc_time)
                .unwrap_or_else(|| "—".to_string()),
            size_text: format_bytes(summary.size_bytes),
            virtual_size_text: summary
                .virtual_size_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".to_string()),
            platform,
            architecture,
            os,
            author: value_or_dash(detail.author.as_deref()),
            docker_version: value_or_dash(detail.docker_version.as_deref()),
            comment: value_or_dash(detail.comment.as_deref()),
            command: string_array(&detail.command),
            entrypoint: string_array(&detail.entrypoint),
            working_dir: value_or_dash(detail.working_dir.as_deref()),
            user: value_or_dash(detail.user.as_deref()),
            stop_signal: value_or_dash(detail.stop_signal.as_deref()),
            shell: string_array(&detail.shell),
            environment,
            labels,
            usage,
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
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

fn string_array(values: &[String]) -> String {
    if values.is_empty() {
        "—".to_string()
    } else {
        serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
    }
}

fn sorted_pairs(values: &BTreeMap<String, String>) -> Vec<KeyValueRow> {
    values
        .iter()
        .map(|(key, value)| KeyValueRow {
            key: key.clone(),
            value: value.clone(),
        })
        .collect()
}

fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "—".to_string()
    } else {
        values.join("\n")
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn value_or_dash(value: Option<&str>) -> String {
    non_empty(value).unwrap_or("—").to_string()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use tuxstack_domain::{ContainerState, EnvironmentVariable};

    use super::*;

    #[test]
    fn creation_time_uses_relative_list_and_explicit_utc_detail_formats() {
        let now = Utc.with_ymd_and_hms(2026, 7, 25, 12, 40, 0).unwrap();
        let created = now - Duration::days(3);
        assert_eq!(relative_time_at(created, now), "3 days ago");
        assert_eq!(
            full_utc_time(Utc.with_ymd_and_hms(2026, 7, 22, 12, 40, 33).unwrap()),
            "Jul 22, 2026 12:40 UTC"
        );
        assert_eq!(
            relative_time_at(now - Duration::minutes(1), now),
            "1 minute ago"
        );
        assert_eq!(
            relative_time_at(now - Duration::hours(1), now),
            "1 hour ago"
        );
        assert_eq!(relative_time_at(now - Duration::days(1), now), "1 day ago");
        assert_eq!(
            relative_time_at(now - Duration::weeks(1), now),
            "7 days ago"
        );
        assert_eq!(
            relative_time_at(now + Duration::minutes(1), now),
            "Jul 25, 2026 12:41 UTC"
        );
    }

    #[test]
    fn detail_view_preserves_tags_platform_environment_labels_and_usage() {
        let created = Utc.with_ymd_and_hms(2026, 7, 22, 12, 40, 33).unwrap();
        let detail = ImageDetail {
            summary: ImageSummary {
                id: "sha256:abcdef".into(),
                short_id: "abcdef".into(),
                repo_tags: vec!["ubuntu:24.04".into(), "ubuntu:latest".into()],
                repo_digests: vec![],
                display_name: "ubuntu:24.04".into(),
                created_at: Some(created),
                size_bytes: 1_322_841_047,
                shared_size_bytes: None,
                virtual_size_bytes: None,
                labels: BTreeMap::new(),
                containers: vec![ImageContainerReference {
                    id: "container-full-id".into(),
                    short_id: "container123".into(),
                    name: "floatctf-dev".into(),
                    state: ContainerState::Exited,
                    status: "Exited (0)".into(),
                    created_at: Some(created),
                }],
                in_use: true,
            },
            architecture: Some("arm64".into()),
            os: Some("linux".into()),
            variant: Some("v8".into()),
            author: None,
            docker_version: None,
            comment: None,
            command: vec![],
            entrypoint: vec![],
            environment: vec![EnvironmentVariable {
                name: "TOKEN".into(),
                value: Some("a=b".into()),
            }],
            working_dir: None,
            user: None,
            stop_signal: None,
            shell: vec![],
            labels: BTreeMap::from([
                ("z.example".into(), "last".into()),
                ("a.example".into(), "first".into()),
            ]),
            root_fs_layers: vec![],
        };

        let view = ImageDetailView::from(&detail);
        assert_eq!(view.repo_tags, vec!["ubuntu:24.04", "ubuntu:latest"]);
        assert_eq!(view.tags_text, "ubuntu:24.04\nubuntu:latest");
        assert!(view.created_text.ends_with("ago") || view.created_text == "2026-07-22");
        assert_eq!(view.created_full_text, "Jul 22, 2026 12:40 UTC");
        assert_eq!(view.platform, "linux/arm64/v8");
        assert_eq!(view.environment[0].key, "TOKEN");
        assert_eq!(view.environment[0].value, "a=b");
        assert_eq!(view.labels[0].key, "a.example");
        assert_eq!(view.labels[1].key, "z.example");
        assert_eq!(view.usage[0].container_id, "container-full-id");
        assert_eq!(view.usage[0].short_id, "container123");
        assert_eq!(view.usage[0].name, "floatctf-dev");
        assert_eq!(view.usage[0].state, "exited");
        assert_eq!(view.usage[0].status, "Exited (0)");
    }
}
