//! Pure mapping helpers for Docker volume DTOs.

use std::collections::{BTreeMap, HashMap};

use bollard::models::{ContainerInspectResponse, ContainerSummary, MountPoint, Volume};
use serde_json::Value;

use crate::models::{
    ContainerState, VolumeContainerReference, VolumeDetail, VolumeSummary, VolumeUsage,
    looks_anonymous_volume,
};

/// Map a Docker volume. A supplied `/system/df` value takes precedence over
/// the value embedded in list/inspect; both correctly preserve unknowns.
pub(crate) fn map_volume_summary(
    volume: Volume,
    system_usage: Option<VolumeUsage>,
) -> VolumeSummary {
    let labels: BTreeMap<_, _> = volume.labels.into_iter().collect();
    let embedded_usage = volume.usage_data.map(|usage| VolumeUsage {
        size_bytes: non_negative(usage.size),
        ref_count: non_negative(usage.ref_count),
    });
    let usage = merge_usage(system_usage, embedded_usage);
    let name = volume.name;

    VolumeSummary {
        anonymous: looks_anonymous_volume(&name, &labels),
        name,
        driver: volume.driver,
        scope: volume
            .scope
            .map(|scope| scope.to_string())
            .filter(|scope| !scope.is_empty())
            .unwrap_or_else(|| "local".to_string()),
        mountpoint: non_empty(volume.mountpoint),
        created_at: volume.created_at,
        labels,
        options: volume.options.into_iter().collect(),
        usage,
        used_by: Vec::new(),
    }
}

pub(crate) fn map_volume_detail(
    volume: Volume,
    system_usage: Option<VolumeUsage>,
    used_by: Vec<VolumeContainerReference>,
) -> VolumeDetail {
    // Bollard 0.21's generated API model represents plugin Status as its
    // keys only (`Option<Vec<String>>`). Preserve those keys rather than
    // inventing values that the client schema discarded.
    let status = volume
        .status
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|key| (key, String::new()))
        .collect();
    let mut summary = map_volume_summary(volume, system_usage);
    summary.used_by = used_by;
    VolumeDetail { summary, status }
}

/// Parse volume items from Bollard 0.21's forward-compatible `Items` values.
/// Any missing, negative, or schema-incompatible usage is omitted so callers
/// report Unknown instead of a fabricated zero.
pub(crate) fn map_system_df_volume_usage(
    items: Option<Vec<Value>>,
) -> HashMap<String, VolumeUsage> {
    items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let name = object.get("Name")?.as_str()?.to_string();
            let usage = object.get("UsageData")?.as_object()?;
            Some((
                name,
                VolumeUsage {
                    size_bytes: json_non_negative(usage.get("Size")),
                    ref_count: json_non_negative(usage.get("RefCount")),
                },
            ))
        })
        .collect()
}

/// Extract only named-volume mounts. Bind, tmpfs, image, npipe, and cluster
/// mounts are deliberately excluded.
pub(crate) fn references_from_summary(
    container: &ContainerSummary,
) -> Vec<(String, VolumeContainerReference)> {
    references(
        container.id.as_deref(),
        container.names.as_deref(),
        container.state.as_ref().map(ToString::to_string).as_deref(),
        container.mounts.as_deref(),
    )
}

pub(crate) fn references_from_inspect(
    container: &ContainerInspectResponse,
) -> Vec<(String, VolumeContainerReference)> {
    let name = container.name.as_deref().map(|name| vec![name.to_string()]);
    let state = container
        .state
        .as_ref()
        .and_then(|state| state.status.as_ref())
        .map(ToString::to_string);
    references(
        container.id.as_deref(),
        name.as_deref(),
        state.as_deref(),
        container.mounts.as_deref(),
    )
}

fn references(
    id: Option<&str>,
    names: Option<&[String]>,
    state: Option<&str>,
    mounts: Option<&[MountPoint]>,
) -> Vec<(String, VolumeContainerReference)> {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return Vec::new();
    };
    let name = names
        .and_then(|names| names.first())
        .map(|name| name.trim_start_matches('/').to_string())
        .unwrap_or_else(|| id.chars().take(12).collect());
    let short_id: String = id.chars().take(12).collect();
    let state = ContainerState::from_str_opt(state.unwrap_or_default());

    mounts
        .unwrap_or_default()
        .iter()
        .filter(|mount| mount.typ.as_deref() == Some("volume"))
        .filter_map(|mount| {
            let volume_name = mount.name.as_deref().filter(|name| !name.is_empty())?;
            Some((
                volume_name.to_string(),
                VolumeContainerReference {
                    id: id.to_string(),
                    short_id: short_id.clone(),
                    name: name.clone(),
                    state,
                    destination: mount.destination.clone().unwrap_or_default(),
                    read_only: !mount.rw.unwrap_or(true),
                    propagation: mount.propagation.clone().filter(|value| !value.is_empty()),
                },
            ))
        })
        .collect()
}

fn merge_usage(primary: Option<VolumeUsage>, fallback: Option<VolumeUsage>) -> VolumeUsage {
    VolumeUsage {
        size_bytes: primary
            .and_then(|usage| usage.size_bytes)
            .or_else(|| fallback.and_then(|usage| usage.size_bytes)),
        ref_count: primary
            .and_then(|usage| usage.ref_count)
            .or_else(|| fallback.and_then(|usage| usage.ref_count)),
    }
}

fn non_negative(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

fn json_non_negative(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_i64).and_then(non_negative)
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use bollard::models::{ContainerSummaryStateEnum, VolumeUsageData};
    use chrono::{TimeZone, Utc};

    use super::*;

    fn raw_volume() -> Volume {
        Volume {
            name: "pgdata".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/pgdata/_data".into(),
            created_at: Some(Utc.with_ymd_and_hms(2026, 3, 19, 15, 10, 0).unwrap()),
            labels: [("app".to_string(), "postgres".to_string())]
                .into_iter()
                .collect(),
            options: [("type".to_string(), "ext4".to_string())]
                .into_iter()
                .collect(),
            usage_data: Some(VolumeUsageData {
                size: 1024,
                ref_count: 1,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn maps_summary_and_inspect_fields() {
        let mapped = map_volume_summary(raw_volume(), None);
        assert_eq!(mapped.name, "pgdata");
        assert_eq!(mapped.scope, "local");
        assert_eq!(
            mapped.mountpoint.as_deref(),
            Some("/var/lib/docker/volumes/pgdata/_data")
        );
        assert_eq!(
            mapped.created_at.unwrap().to_rfc3339(),
            "2026-03-19T15:10:00+00:00"
        );
        assert_eq!(mapped.usage.size_bytes, Some(1024));
        assert_eq!(mapped.usage.ref_count, Some(1));
        assert_eq!(
            mapped.labels.get("app").map(String::as_str),
            Some("postgres")
        );
        assert_eq!(mapped.options.get("type").map(String::as_str), Some("ext4"));
        assert!(!mapped.anonymous);

        let detail = map_volume_detail(raw_volume(), None, Vec::new());
        assert!(detail.status.is_empty());
    }

    #[test]
    fn negative_and_missing_usage_stay_unknown() {
        let mut volume = raw_volume();
        volume.usage_data = Some(VolumeUsageData {
            size: -1,
            ref_count: -1,
        });
        let mapped = map_volume_summary(volume, None);
        assert_eq!(mapped.usage, VolumeUsage::default());

        let mut volume = raw_volume();
        volume.usage_data = None;
        assert_eq!(
            map_volume_summary(volume, None).usage,
            VolumeUsage::default()
        );
    }

    #[test]
    fn empty_labels_options_status_and_mountpoint_map_cleanly() {
        let mapped = map_volume_detail(Volume::default(), None, Vec::new());
        assert!(mapped.summary.labels.is_empty());
        assert!(mapped.summary.options.is_empty());
        assert!(mapped.status.is_empty());
        assert!(mapped.summary.mountpoint.is_none());
        assert!(mapped.summary.used_by.is_empty());
        assert!(!mapped.summary.in_use());
    }

    #[test]
    fn df_usage_is_opportunistic_and_schema_safe() {
        let usage = map_system_df_volume_usage(Some(vec![
            serde_json::json!({"Name":"known","UsageData":{"Size":42,"RefCount":2}}),
            serde_json::json!({"Name":"unknown","UsageData":{"Size":-1,"RefCount":-1}}),
            serde_json::json!({"unexpected":"schema"}),
        ]));
        assert_eq!(usage["known"].size_bytes, Some(42));
        assert_eq!(usage["unknown"], VolumeUsage::default());
        assert_eq!(usage.len(), 2);
    }

    fn raw_container(
        state: ContainerSummaryStateEnum,
        mounts: Vec<MountPoint>,
    ) -> ContainerSummary {
        ContainerSummary {
            id: Some("0123456789abcdef".into()),
            names: Some(vec!["/postgres".into()]),
            state: Some(state),
            mounts: Some(mounts),
            ..Default::default()
        }
    }

    fn mount(typ: &str, name: Option<&str>, destination: &str, rw: bool) -> MountPoint {
        MountPoint {
            typ: Some(typ.into()),
            name: name.map(str::to_string),
            destination: Some(destination.into()),
            rw: Some(rw),
            propagation: Some("rprivate".into()),
            ..Default::default()
        }
    }

    #[test]
    fn associates_running_stopped_and_paused_containers() {
        for state in [
            ContainerSummaryStateEnum::RUNNING,
            ContainerSummaryStateEnum::EXITED,
            ContainerSummaryStateEnum::PAUSED,
        ] {
            let refs = references_from_summary(&raw_container(
                state,
                vec![mount("volume", Some("pgdata"), "/data", true)],
            ));
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].0, "pgdata");
            assert_eq!(refs[0].1.destination, "/data");
        }
    }

    #[test]
    fn excludes_bind_and_tmpfs_and_maps_read_only() {
        let refs = references_from_summary(&raw_container(
            ContainerSummaryStateEnum::RUNNING,
            vec![
                mount("bind", None, "/bind", true),
                mount("tmpfs", None, "/tmp", true),
                mount("volume", Some("one"), "/one", false),
                mount("volume", Some("two"), "/two", true),
            ],
        ));
        assert_eq!(refs.len(), 2);
        assert!(refs[0].1.read_only);
        assert_eq!(refs[0].1.propagation.as_deref(), Some("rprivate"));
        assert!(!refs[1].1.read_only);
    }

    #[test]
    fn multiple_containers_can_reference_the_same_volume() {
        let first = raw_container(
            ContainerSummaryStateEnum::RUNNING,
            vec![mount("volume", Some("shared"), "/a", true)],
        );
        let mut second = raw_container(
            ContainerSummaryStateEnum::EXITED,
            vec![mount("volume", Some("shared"), "/b", true)],
        );
        second.id = Some("fedcba9876543210".into());
        let all: Vec<_> = [first, second]
            .iter()
            .flat_map(references_from_summary)
            .collect();
        assert_eq!(all.iter().filter(|(name, _)| name == "shared").count(), 2);
    }
}
