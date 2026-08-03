//! Mapping for image DTOs.

use bollard::models::ImageSummary as BollardImageSummary;

use crate::mapping::containers::{from_unix_seconds, short_id};
use crate::models::ImageSummary;

/// Map a bollard image list entry into the domain model.
pub fn map_image_summary(image: BollardImageSummary) -> ImageSummary {
    ImageSummary {
        id: image.id.clone(),
        short_id: short_id(&image.id),
        repository_tags: image.repo_tags.clone(),
        repository_digests: image.repo_digests.clone(),
        created_at: from_unix_seconds(image.created),
        size_bytes: image.size.max(0) as u64,
        virtual_size_bytes: (image.size + image.shared_size).max(0) as u64,
        containers: image.containers.max(0) as u64,
        labels: image.labels.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BollardImageSummary {
        BollardImageSummary {
            id: "sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".into(),
            repo_tags: vec!["nginx:latest".into(), "nginx:1.27".into()],
            repo_digests: vec!["sha256:abc@sha256:def".into()],
            created: 1_700_000_000,
            size: 100_000_000,
            shared_size: 20_000_000,
            containers: 3,
            labels: Default::default(),
            ..Default::default()
        }
    }

    #[test]
    fn maps_all_fields() {
        let mapped = map_image_summary(sample());
        assert_eq!(mapped.repository_tags, vec!["nginx:latest", "nginx:1.27"]);
        assert_eq!(mapped.created_at.timestamp(), 1_700_000_000);
        assert_eq!(mapped.size_bytes, 100_000_000);
        assert_eq!(mapped.virtual_size_bytes, 120_000_000);
        assert_eq!(mapped.containers, 3);
        assert_eq!(mapped.primary_tag(), "nginx:latest");
        assert_eq!(mapped.short_id.len(), 12);
    }

    #[test]
    fn negative_sizes_clamp_to_zero() {
        let mut image = sample();
        image.size = -5;
        let mapped = map_image_summary(image);
        assert_eq!(mapped.size_bytes, 0);
    }

    #[test]
    fn no_tags_uses_none_placeholder() {
        let mut image = sample();
        image.repo_tags = vec![];
        let mapped = map_image_summary(image);
        assert_eq!(mapped.primary_tag(), "<none>");
    }
}
