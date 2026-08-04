//! Pure Docker volume view models used by the Qt bridge.
//!
//! These types contain no Qt values and deliberately expose structured rows
//! instead of serializing a `VolumeDetail` as JSON.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use tuxstack_docker_core::{VolumeContainerReference, VolumeDetail, VolumeSummary};

/// One row in the volume list. Dynamic selection/operation fields are filled
/// by `VolumesState`, while all Docker-derived fields come from the summary.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeRow {
    pub volume_name: String,
    pub display_name: String,
    pub driver: String,
    pub scope: String,
    pub mountpoint: String,
    pub size_bytes: Option<u64>,
    pub size_known: bool,
    pub size_text: String,
    pub created_at: Option<DateTime<Utc>>,
    pub created_text: String,
    pub in_use: bool,
    pub used_by_count: usize,
    pub anonymous: bool,
    pub selected: bool,
    pub busy: bool,
    pub operation: String,
    pub section: String,
    pub labels: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
    pub containers: Vec<VolumeContainerReference>,
}

impl From<&VolumeSummary> for VolumeRow {
    fn from(summary: &VolumeSummary) -> Self {
        let used_by_count = summary.used_by.len();
        let size_bytes = summary.usage.size_bytes;
        let size_text = size_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown size".to_string());
        Self {
            volume_name: summary.name.clone(),
            display_name: display_volume_name(&summary.name, summary.anonymous),
            driver: summary.driver.clone(),
            scope: summary.scope.clone(),
            mountpoint: summary.mountpoint.clone().unwrap_or_default(),
            size_bytes,
            size_known: size_bytes.is_some(),
            size_text: size_text.clone(),
            created_at: summary.created_at,
            created_text: summary.created_at.map(full_utc_time).unwrap_or_else(dash),
            in_use: used_by_count != 0,
            used_by_count,
            anonymous: summary.anonymous,
            selected: false,
            busy: false,
            operation: String::new(),
            section: if summary.used_by.is_empty() {
                "unused".to_string()
            } else {
                "in_use".to_string()
            },
            labels: summary.labels.clone(),
            options: summary.options.clone(),
            containers: summary.used_by.clone(),
        }
    }
}

impl VolumeRow {
    pub fn secondary_text(&self) -> String {
        let use_text = match self.used_by_count {
            0 => "Unused".to_string(),
            1 => "1 container".to_string(),
            count => format!("{count} containers"),
        };
        format!("{} · {use_text}", self.size_text)
    }
}

/// Aggregate known/unknown size information. Unknown is never represented as
/// zero and therefore cannot make the list header misleading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VolumeSizeSummary {
    pub known_total_bytes: u64,
    pub known_count: usize,
    pub unknown_count: usize,
}

impl VolumeSizeSummary {
    pub fn from_rows(rows: &[VolumeRow]) -> Self {
        let mut result = Self::default();
        for row in rows {
            match row.size_bytes {
                Some(bytes) => {
                    result.known_total_bytes = result.known_total_bytes.saturating_add(bytes);
                    result.known_count += 1;
                }
                None => result.unknown_count += 1,
            }
        }
        result
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        match (self.known_count, self.unknown_count) {
            (0, _) => "Volume sizes unavailable".to_string(),
            (_, 0) => format!("{} total volume data", format_bytes(self.known_total_bytes)),
            (_, 1) => format!(
                "{} known · 1 volume unknown",
                format_bytes(self.known_total_bytes)
            ),
            (_, unknown) => format!(
                "{} known · {unknown} volumes unknown",
                format_bytes(self.known_total_bytes)
            ),
        }
    }
}

/// A safe key/value row for Labels, Driver Options, and plugin Status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeKeyValueRow {
    pub key: String,
    pub value: String,
}

/// One General-section row. `copyable` lets QML offer copying only where it is
/// useful, without interpreting labels or optional Rust values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumePropertyRow {
    pub label: String,
    pub value: String,
    pub copyable: bool,
}

/// One container in the Used By section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeContainerView {
    pub container_id: String,
    pub short_id: String,
    pub name: String,
    pub state: String,
    pub destination: String,
    pub read_only: bool,
    pub access_text: String,
    pub propagation: String,
}

impl From<&VolumeContainerReference> for VolumeContainerView {
    fn from(reference: &VolumeContainerReference) -> Self {
        Self {
            container_id: reference.id.clone(),
            short_id: reference.short_id.clone(),
            name: reference.name.clone(),
            state: reference.state.as_str().to_string(),
            destination: value_or_dash(Some(&reference.destination)),
            read_only: reference.read_only,
            access_text: if reference.read_only {
                "Read Only".to_string()
            } else {
                "Read/Write".to_string()
            },
            propagation: value_or_dash(reference.propagation.as_deref()),
        }
    }
}

/// Scalar and structured detail values for the permanent detail panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeDetailView {
    pub volume_name: String,
    pub display_name: String,
    pub driver: String,
    pub scope: String,
    pub mountpoint: String,
    pub created_text: String,
    pub size_bytes: Option<u64>,
    pub size_known: bool,
    pub size_text: String,
    pub ref_count: Option<u64>,
    pub ref_count_text: String,
    pub anonymous: bool,
    pub anonymous_text: String,
    pub general: Vec<VolumePropertyRow>,
    pub used_by: Vec<VolumeContainerView>,
    pub labels: Vec<VolumeKeyValueRow>,
    pub options: Vec<VolumeKeyValueRow>,
    pub status: Vec<VolumeKeyValueRow>,
}

impl From<&VolumeDetail> for VolumeDetailView {
    fn from(detail: &VolumeDetail) -> Self {
        let summary = &detail.summary;
        let mountpoint = value_or_dash(summary.mountpoint.as_deref());
        let created_text = summary.created_at.map(full_utc_time).unwrap_or_else(dash);
        let size_text = summary
            .usage
            .size_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unknown".to_string());
        let ref_count_text = summary
            .usage
            .ref_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let anonymous_text = if summary.anonymous { "Yes" } else { "No" }.to_string();
        let mut used_by: Vec<_> = summary.used_by.iter().map(Into::into).collect();
        used_by.sort_by(|left: &VolumeContainerView, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.container_id.cmp(&right.container_id))
                .then_with(|| left.destination.cmp(&right.destination))
        });

        let general = vec![
            property("Name", &summary.name, true),
            property("Driver", value_or_dash(Some(&summary.driver)), false),
            property("Scope", value_or_dash(Some(&summary.scope)), false),
            property("Mountpoint", &mountpoint, true),
            property("Created", &created_text, false),
            property("Size", &size_text, false),
            property("Reference Count", &ref_count_text, false),
            property("Anonymous", &anonymous_text, false),
        ];

        Self {
            volume_name: summary.name.clone(),
            display_name: display_volume_name(&summary.name, summary.anonymous),
            driver: value_or_dash(Some(&summary.driver)),
            scope: value_or_dash(Some(&summary.scope)),
            mountpoint,
            created_text,
            size_bytes: summary.usage.size_bytes,
            size_known: summary.usage.size_bytes.is_some(),
            size_text,
            ref_count: summary.usage.ref_count,
            ref_count_text,
            anonymous: summary.anonymous,
            anonymous_text,
            general,
            used_by,
            labels: sorted_pairs(&summary.labels),
            options: sorted_pairs(&summary.options),
            status: sorted_pairs(&detail.status),
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

fn display_volume_name(name: &str, anonymous: bool) -> String {
    if anonymous && name.chars().count() > 15 {
        format!("{}…", name.chars().take(12).collect::<String>())
    } else {
        name.to_string()
    }
}

fn sorted_pairs(values: &BTreeMap<String, String>) -> Vec<VolumeKeyValueRow> {
    let mut rows: Vec<_> = values
        .iter()
        .map(|(key, value)| VolumeKeyValueRow {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();
    rows.sort_by(|left, right| {
        left.key
            .to_lowercase()
            .cmp(&right.key.to_lowercase())
            .then_with(|| left.key.cmp(&right.key))
    });
    rows
}

fn property(label: &str, value: impl AsRef<str>, copyable: bool) -> VolumePropertyRow {
    VolumePropertyRow {
        label: label.to_string(),
        value: value.as_ref().to_string(),
        copyable,
    }
}

fn value_or_dash(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn dash() -> String {
    "—".to_string()
}

fn full_utc_time(created: DateTime<Utc>) -> String {
    created.format("%b %-d, %Y %H:%M UTC").to_string()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tuxstack_docker_core::{ContainerState, VolumeUsage};

    use super::*;

    fn reference(name: &str, state: ContainerState) -> VolumeContainerReference {
        VolumeContainerReference {
            id: format!("{name}-full-id"),
            short_id: format!("{name}-short"),
            name: name.to_string(),
            state,
            destination: format!("/srv/{name}"),
            read_only: name == "readonly",
            propagation: None,
        }
    }

    fn summary(name: &str, size: Option<u64>) -> VolumeSummary {
        VolumeSummary {
            name: name.to_string(),
            driver: "local".to_string(),
            scope: "local".to_string(),
            mountpoint: Some(format!("/var/lib/docker/volumes/{name}/_data")),
            created_at: Some(Utc.with_ymd_and_hms(2026, 3, 19, 15, 10, 0).unwrap()),
            labels: BTreeMap::from([
                ("z.example".to_string(), "last".to_string()),
                ("A.example".to_string(), "first".to_string()),
            ]),
            options: BTreeMap::from([("type".to_string(), "nfs".to_string())]),
            usage: VolumeUsage {
                size_bytes: size,
                ref_count: Some(1),
            },
            used_by: vec![reference("postgres", ContainerState::Exited)],
            anonymous: false,
        }
    }

    #[test]
    fn list_row_preserves_unknown_size_and_stopped_container_usage() {
        let row = VolumeRow::from(&summary("pgdata", None));
        assert_eq!(row.volume_name, "pgdata");
        assert_eq!(row.size_bytes, None);
        assert!(!row.size_known);
        assert_eq!(row.size_text, "Unknown size");
        assert_eq!(row.secondary_text(), "Unknown size · 1 container");
        assert!(row.in_use);
        assert_eq!(row.section, "in_use");
        assert_eq!(row.containers[0].state, ContainerState::Exited);
    }

    #[test]
    fn size_summary_never_calls_unknown_zero() {
        let rows = [
            VolumeRow::from(&summary("known", Some(1536))),
            VolumeRow::from(&summary("unknown-a", None)),
            VolumeRow::from(&summary("unknown-b", None)),
        ];
        let summary = VolumeSizeSummary::from_rows(&rows);
        assert_eq!(summary.known_total_bytes, 1536);
        assert_eq!(summary.known_count, 1);
        assert_eq!(summary.unknown_count, 2);
        assert_eq!(summary.text(), "1.5 KiB known · 2 volumes unknown");

        let all_unknown = VolumeSizeSummary::from_rows(&rows[1..]);
        assert_eq!(all_unknown.text(), "Volume sizes unavailable");
        assert!(!all_unknown.text().contains("0 B"));
    }

    #[test]
    fn size_summary_saturates_and_formats_fully_known_data() {
        let rows = [
            VolumeRow::from(&summary("a", Some(u64::MAX))),
            VolumeRow::from(&summary("b", Some(2))),
        ];
        let aggregate = VolumeSizeSummary::from_rows(&rows);
        assert_eq!(aggregate.known_total_bytes, u64::MAX);
        assert_eq!(aggregate.known_count, 2);
        assert_eq!(aggregate.unknown_count, 0);
        assert!(aggregate.text().ends_with("total volume data"));
    }

    #[test]
    fn detail_view_is_structured_sorted_and_has_safe_placeholders() {
        let mut source = summary("pgdata", None);
        source.mountpoint = None;
        source.created_at = None;
        source.usage.ref_count = None;
        source.used_by = vec![
            reference("web", ContainerState::Running),
            reference("readonly", ContainerState::Paused),
        ];
        let detail = VolumeDetail {
            summary: source,
            status: BTreeMap::from([
                ("z.status".to_string(), "last".to_string()),
                ("a.status".to_string(), "first".to_string()),
            ]),
        };
        let view = VolumeDetailView::from(&detail);
        assert_eq!(view.mountpoint, "—");
        assert_eq!(view.created_text, "—");
        assert_eq!(view.size_text, "Unknown");
        assert_eq!(view.ref_count_text, "Unknown");
        assert_eq!(view.general.len(), 8);
        assert_eq!(view.labels[0].key, "A.example");
        assert_eq!(view.status[0].key, "a.status");
        assert_eq!(view.used_by[0].name, "readonly");
        assert_eq!(view.used_by[0].access_text, "Read Only");
        assert_eq!(view.used_by[1].access_text, "Read/Write");
        assert!(!format!("{view:?}").contains("Some("));
    }

    #[test]
    fn anonymous_display_is_conservative_and_keeps_full_identity() {
        let name = "a".repeat(64);
        let mut source = summary(&name, Some(0));
        source.anonymous = true;
        source.used_by.clear();
        let row = VolumeRow::from(&source);
        assert_eq!(row.volume_name, name);
        assert_eq!(row.display_name, "aaaaaaaaaaaa…");
        assert_eq!(row.section, "unused");
        assert_eq!(row.secondary_text(), "0 B · Unused");
    }
}
