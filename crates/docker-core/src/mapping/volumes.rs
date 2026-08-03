//! Mapping for volume DTOs.

use bollard::models::Volume as BollardVolume;

use crate::models::VolumeSummary;

/// Map a bollard volume into the domain model.
pub fn map_volume_summary(volume: BollardVolume) -> VolumeSummary {
    VolumeSummary {
        name: volume.name,
        driver: volume.driver,
        mountpoint: volume.mountpoint,
        scope: volume
            .scope
            .map(|s| s.to_string())
            .unwrap_or_else(|| "local".to_string()),
        created_at: volume.created_at,
        labels: volume.labels.into_iter().collect(),
        options: volume.options.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_fields() {
        let v = BollardVolume {
            name: "pgdata".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/pgdata/_data".into(),
            created_at: None,
            labels: [("app".to_string(), "postgres".to_string())]
                .into_iter()
                .collect(),
            options: [("type".to_string(), "ext4".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mapped = map_volume_summary(v);
        assert_eq!(mapped.name, "pgdata");
        assert_eq!(mapped.mountpoint, "/var/lib/docker/volumes/pgdata/_data");
        assert_eq!(mapped.scope, "local");
        assert_eq!(
            mapped.labels.get("app").map(|s| s.as_str()),
            Some("postgres")
        );
    }

    #[test]
    fn empty_volume_does_not_panic() {
        let mapped = map_volume_summary(BollardVolume::default());
        assert_eq!(mapped.name, "");
        assert_eq!(mapped.scope, "local");
    }
}
