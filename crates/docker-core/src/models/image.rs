//! Image domain models.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ContainerState, EnvironmentVariable};

/// A unique local image and the containers which reference it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageSummary {
    /// Canonical image ID (`sha256:<digest>` when Docker returned a digest).
    pub id: String,
    pub short_id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub display_name: String,
    pub created_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
    pub shared_size_bytes: Option<u64>,
    pub virtual_size_bytes: Option<u64>,
    pub labels: BTreeMap<String, String>,
    pub containers: Vec<ImageContainerReference>,
    pub in_use: bool,
}

/// Split a tagged image reference at its final tag separator.
///
/// Registry ports are preserved (`registry:5000/repo:tag`), while digest-only,
/// untagged, and dangling references return `None`.
pub fn parse_repo_tag(reference: &str) -> Option<(&str, &str)> {
    if reference.is_empty() || reference == "<none>:<none>" || reference.contains('@') {
        return None;
    }
    let slash = reference.rfind('/');
    let colon = reference.rfind(':')?;
    if slash.is_some_and(|slash| colon < slash) {
        return None;
    }
    let (repository, tag_with_colon) = reference.split_at(colon);
    let tag = &tag_with_colon[1..];
    (!repository.is_empty() && !tag.is_empty()).then_some((repository, tag))
}

impl ImageSummary {
    /// Sum logical image sizes once per normalized image ID.
    pub fn total_unique_size(images: &[Self]) -> u64 {
        let mut seen = std::collections::HashSet::new();
        images
            .iter()
            .filter(|image| seen.insert(crate::mapping::images::normalize_image_id(&image.id)))
            .map(|image| image.size_bytes)
            .sum()
    }

    /// The primary tag, falling back to Docker's dangling-image placeholder.
    pub fn primary_tag(&self) -> &str {
        self.repo_tags
            .first()
            .map(String::as_str)
            .unwrap_or("<none>:<none>")
    }

    /// Compatibility accessor for callers which used the old field name.
    pub fn repository_tags(&self) -> &[String] {
        &self.repo_tags
    }

    /// Compatibility accessor for callers which used the old field name.
    pub fn repository_digests(&self) -> &[String] {
        &self.repo_digests
    }
}

/// Minimal information about a container which references an image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageContainerReference {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub state: ContainerState,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
}

/// Fully inspected image information. No Bollard DTO is exposed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageDetail {
    pub summary: ImageSummary,
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub variant: Option<String>,
    pub author: Option<String>,
    pub docker_version: Option<String>,
    pub comment: Option<String>,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub environment: Vec<EnvironmentVariable>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub stop_signal: Option<String>,
    pub shell: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub root_fs_layers: Vec<String>,
}

/// Authentication used for one registry request.
///
/// Secret fields deliberately use a redacted `Debug` implementation and are
/// not serialized. Callers must also avoid logging the value manually.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryAuth {
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub server_address: Option<String>,
    #[serde(skip_serializing)]
    pub identity_token: Option<String>,
    #[serde(skip_serializing)]
    pub registry_token: Option<String>,
}

impl fmt::Debug for RegistryAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryAuth")
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("server_address", &self.server_address)
            .field(
                "identity_token",
                &self.identity_token.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "registry_token",
                &self.registry_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Parameters for pulling one image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullImageOptions {
    pub reference: String,
    pub platform: Option<String>,
    pub registry_auth: Option<RegistryAuth>,
}

/// Parameters for removing an image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveImageOptions {
    pub force: bool,
    /// Delete untagged parent images as part of the operation.
    pub prune_children: bool,
}

/// One action reported by Docker after image removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageDeleteResult {
    Untagged(String),
    Deleted(String),
}

/// A real update from Docker's image pull stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagePullProgress {
    pub image_reference: String,
    pub layer_id: Option<String>,
    pub status: String,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, size_bytes: u64) -> ImageSummary {
        ImageSummary {
            id: id.into(),
            short_id: id.into(),
            repo_tags: vec![],
            repo_digests: vec![],
            display_name: "<none>:<none>".into(),
            created_at: None,
            size_bytes,
            shared_size_bytes: None,
            virtual_size_bytes: None,
            labels: BTreeMap::new(),
            containers: vec![],
            in_use: false,
        }
    }

    #[test]
    fn parses_repo_tags_without_confusing_registry_ports() {
        assert_eq!(parse_repo_tag("ubuntu:24.04"), Some(("ubuntu", "24.04")));
        assert_eq!(
            parse_repo_tag("registry.example:5000/project/image:v1"),
            Some(("registry.example:5000/project/image", "v1"))
        );
        assert_eq!(parse_repo_tag("registry.example:5000/project/image"), None);
        assert_eq!(parse_repo_tag("image@sha256:abc"), None);
        assert_eq!(parse_repo_tag("<none>:<none>"), None);
    }

    #[test]
    fn total_size_counts_normalized_image_id_once() {
        let images = vec![
            summary("sha256:abcdef123456", 100),
            summary("abcdef123456", 100),
            summary("sha256:other", 25),
        ];
        assert_eq!(ImageSummary::total_unique_size(&images), 125);
    }
}
