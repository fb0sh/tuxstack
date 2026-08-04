//! Docker volume domain models.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A Docker volume together with usage and container-reference information.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeSummary {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub mountpoint: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub labels: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
    pub usage: VolumeUsage,
    pub used_by: Vec<VolumeContainerReference>,
    pub anonymous: bool,
}

impl VolumeSummary {
    /// A volume is in use when any existing container references it. Stopped
    /// containers deliberately count as users.
    pub fn in_use(&self) -> bool {
        !self.used_by.is_empty()
    }
}

impl fmt::Debug for VolumeSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VolumeSummary")
            .field("name", &self.name)
            .field("driver", &self.driver)
            .field("scope", &self.scope)
            .field("mountpoint", &self.mountpoint)
            .field("created_at", &self.created_at)
            .field("label_keys", &self.labels.keys().collect::<Vec<_>>())
            .field("option_keys", &self.options.keys().collect::<Vec<_>>())
            .field("usage", &self.usage)
            .field("used_by", &self.used_by)
            .field("anonymous", &self.anonymous)
            .finish()
    }
}

/// Usage values reported by Docker. `None` means unavailable, not zero.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeUsage {
    pub size_bytes: Option<u64>,
    pub ref_count: Option<u64>,
}

/// Complete volume detail returned by inspect.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeDetail {
    pub summary: VolumeSummary,
    pub status: BTreeMap<String, String>,
}

impl fmt::Debug for VolumeDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VolumeDetail")
            .field("summary", &self.summary)
            .field("status_keys", &self.status.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// One container mount that references a named volume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeContainerReference {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub state: crate::models::ContainerState,
    pub destination: String,
    pub read_only: bool,
    pub propagation: Option<String>,
}

/// Request to create a volume. Empty optional strings are normalized by the
/// service; label and driver-option values are never logged by docker-core.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateVolumeRequest {
    pub name: Option<String>,
    pub driver: Option<String>,
    pub driver_options: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
}

impl fmt::Debug for CreateVolumeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateVolumeRequest")
            .field("name", &self.name)
            .field("driver", &self.driver)
            .field(
                "driver_option_keys",
                &self.driver_options.keys().collect::<Vec<_>>(),
            )
            .field("label_keys", &self.labels.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoveVolumeOptions {
    pub force: bool,
}

/// Raw Docker volume-prune filters. Supported keys are engine-version
/// dependent; common keys are `label` and `all`.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruneVolumeFilters {
    pub filters: BTreeMap<String, Vec<String>>,
}

impl fmt::Debug for PruneVolumeFilters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PruneVolumeFilters")
            .field("filter_keys", &self.filters.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumePruneResult {
    pub volumes_deleted: Vec<String>,
    /// `None` means Docker did not report a non-negative value.
    pub space_reclaimed_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VolumeExportCompression {
    Tar,
    TarGzip,
    TarZstd,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportVolumeRequest {
    pub volume_name: String,
    pub destination: PathBuf,
    pub compression: VolumeExportCompression,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloneVolumeRequest {
    pub source_volume: String,
    pub target_name: String,
    pub target_driver: Option<String>,
    pub target_driver_options: BTreeMap<String, String>,
    pub copy_labels: bool,
}

impl fmt::Debug for CloneVolumeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloneVolumeRequest")
            .field("source_volume", &self.source_volume)
            .field("target_name", &self.target_name)
            .field("target_driver", &self.target_driver)
            .field(
                "target_driver_option_keys",
                &self.target_driver_options.keys().collect::<Vec<_>>(),
            )
            .field("copy_labels", &self.copy_labels)
            .finish()
    }
}

/// Aggregate only known sizes. Unknown values are counted separately and are
/// never represented as zero.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeSizeSummary {
    pub known_total_bytes: u64,
    pub known_count: usize,
    pub unknown_count: usize,
}

impl VolumeSizeSummary {
    pub fn from_volumes(volumes: &[VolumeSummary]) -> Self {
        volumes.iter().fold(Self::default(), |mut result, volume| {
            match volume.usage.size_bytes {
                Some(size) => {
                    result.known_total_bytes = result.known_total_bytes.saturating_add(size);
                    result.known_count += 1;
                }
                None => result.unknown_count += 1,
            }
            result
        })
    }
}

/// Sort modes shared by frontends. Size sorts always put unknown values last.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VolumeSort {
    NameAscending,
    NameDescending,
    NewestFirst,
    OldestFirst,
    LargestFirst,
    SmallestFirst,
    MostContainers,
    FewestContainers,
    InUseFirst,
    UnusedFirst,
}

/// Sort volumes in place with deterministic name tie-breaking.
pub fn sort_volumes(volumes: &mut [VolumeSummary], sort: VolumeSort) {
    volumes.sort_by(|left, right| {
        let by_name = || left.name.cmp(&right.name);
        let ordering = match sort {
            VolumeSort::NameAscending => by_name(),
            VolumeSort::NameDescending => right.name.cmp(&left.name),
            VolumeSort::NewestFirst => option_last_cmp_desc(left.created_at, right.created_at),
            VolumeSort::OldestFirst => option_last_cmp(left.created_at, right.created_at),
            VolumeSort::LargestFirst => {
                option_last_cmp_desc(left.usage.size_bytes, right.usage.size_bytes)
            }
            VolumeSort::SmallestFirst => {
                option_last_cmp(left.usage.size_bytes, right.usage.size_bytes)
            }
            VolumeSort::MostContainers => right.used_by.len().cmp(&left.used_by.len()),
            VolumeSort::FewestContainers => left.used_by.len().cmp(&right.used_by.len()),
            VolumeSort::InUseFirst => right.in_use().cmp(&left.in_use()),
            VolumeSort::UnusedFirst => left.in_use().cmp(&right.in_use()),
        };
        ordering.then_with(by_name)
    });
}

fn option_last_cmp<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn option_last_cmp_desc<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Conservative anonymous-volume heuristic. Any label indicates deliberate
/// management; otherwise only Docker's common 64-hex generated form matches.
pub fn looks_anonymous_volume(name: &str, labels: &BTreeMap<String, String>) -> bool {
    labels.is_empty() && name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(name: &str, size: Option<u64>) -> VolumeSummary {
        VolumeSummary {
            name: name.into(),
            driver: "local".into(),
            scope: "local".into(),
            mountpoint: None,
            created_at: None,
            labels: BTreeMap::new(),
            options: BTreeMap::new(),
            usage: VolumeUsage {
                size_bytes: size,
                ref_count: None,
            },
            used_by: Vec::new(),
            anonymous: false,
        }
    }

    #[test]
    fn size_summary_never_turns_unknown_into_zero() {
        let summary = VolumeSizeSummary::from_volumes(&[
            volume("one", Some(10)),
            volume("two", None),
            volume("three", Some(25)),
        ]);
        assert_eq!(summary.known_total_bytes, 35);
        assert_eq!(summary.known_count, 2);
        assert_eq!(summary.unknown_count, 1);
    }

    #[test]
    fn unknown_size_sorts_last_in_both_directions() {
        for sort in [VolumeSort::LargestFirst, VolumeSort::SmallestFirst] {
            let mut volumes = vec![
                volume("unknown", None),
                volume("large", Some(100)),
                volume("small", Some(1)),
            ];
            sort_volumes(&mut volumes, sort);
            assert_eq!(volumes.last().unwrap().name, "unknown");
        }
    }

    #[test]
    fn anonymous_detection_is_conservative() {
        let hex = "a".repeat(64);
        assert!(looks_anonymous_volume(&hex, &BTreeMap::new()));
        assert!(!looks_anonymous_volume("a-short-name", &BTreeMap::new()));
        assert!(!looks_anonymous_volume(
            &hex,
            &[("com.docker.compose.volume".into(), "data".into())]
                .into_iter()
                .collect()
        ));
    }

    #[test]
    fn debug_output_redacts_label_and_driver_option_values() {
        let secret = "do-not-log-this-value";
        let request = CreateVolumeRequest {
            labels: [("label".into(), secret.into())].into_iter().collect(),
            driver_options: [("password".into(), secret.into())].into_iter().collect(),
            ..Default::default()
        };
        assert!(!format!("{request:?}").contains(secret));

        let mut summary = volume("safe", None);
        summary.labels.insert("label".into(), secret.into());
        summary.options.insert("password".into(), secret.into());
        assert!(!format!("{summary:?}").contains(secret));
    }
}
