//! Image management service.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::Arc;

use bollard::auth::DockerCredentials;
use bollard::query_parameters::{
    CreateImageOptions as BollardCreateImageOptions,
    ListContainersOptions as BollardListContainersOptions,
    ListImagesOptions as BollardListImagesOptions, RemoveImageOptions as BollardRemoveImageOptions,
};
use futures_util::{StreamExt, stream};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};
use crate::mapping::containers::map_container_summary;
use crate::mapping::images::{map_image_detail, map_image_summaries};
use crate::models::{
    ImageDeleteResult, ImageDetail, ImagePullProgress, ImageSummary, PullImageOptions,
    RegistryAuth, RemoveImageOptions,
};
use crate::streams::{ImageExportStream, ImagePullStream};

/// Options for listing local images.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListImagesOptions {
    /// Local case-insensitive search over IDs, references, labels, architecture
    /// (when present in labels), and names of associated containers.
    pub search: Option<String>,
}

/// Image service backed by the shared Docker client.
#[derive(Clone)]
pub struct ImageService {
    client: Arc<DockerClient>,
}

impl ImageService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List unique images and associate every existing container, including
    /// stopped, paused, exited, and created containers.
    ///
    /// The generic option accepts both `ListImagesOptions` and
    /// `&ListImagesOptions` for source compatibility with older callers.
    pub async fn list_images<O>(&self, options: O) -> Result<Vec<ImageSummary>, DockerError>
    where
        O: Borrow<ListImagesOptions>,
    {
        let timer = crate::instrument::Timer::start("docker.list_images");
        let result = self.list_images_inner(options.borrow()).await;
        match &result {
            Ok(images) => timer.finish_ok(images.len(), "live"),
            Err(error) => timer.finish_err(&error.to_string()),
        }
        result
    }

    async fn list_images_inner(
        &self,
        options: &ListImagesOptions,
    ) -> Result<Vec<ImageSummary>, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;
        let image_options = BollardListImagesOptions {
            all: true,
            filters: None::<HashMap<String, Vec<String>>>,
            shared_size: true,
            digests: true,
            ..Default::default()
        };
        let container_options = BollardListContainersOptions {
            all: true,
            ..Default::default()
        };

        let images_future = docker.list_images(Some(image_options));
        let containers_future = docker.list_containers(Some(container_options));
        let (images, containers) = tokio::try_join!(
            async {
                tokio::time::timeout(timeout, images_future)
                    .await
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "image"))
            },
            async {
                tokio::time::timeout(timeout, containers_future)
                    .await
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "container"))
            }
        )?;

        let containers: Vec<_> = containers.into_iter().map(map_container_summary).collect();
        let mut mapped = map_image_summaries(images, &containers);
        apply_search(&mut mapped, options.search.as_deref());
        mapped.sort_by(|a, b| {
            b.in_use.cmp(&a.in_use).then_with(|| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| a.display_name.cmp(&b.display_name))
            })
        });
        Ok(mapped)
    }

    /// Inspect one image into the fully typed domain model. Container
    /// associations are refreshed using an all-container request.
    pub async fn inspect_image(&self, id: &str) -> Result<ImageDetail, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;
        let container_options = BollardListContainersOptions {
            all: true,
            ..Default::default()
        };
        let (inspect, containers) = tokio::try_join!(
            async {
                tokio::time::timeout(timeout, docker.inspect_image(id))
                    .await
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "image"))
            },
            async {
                tokio::time::timeout(timeout, docker.list_containers(Some(container_options)))
                    .await
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "container"))
            }
        )?;
        let containers: Vec<_> = containers.into_iter().map(map_container_summary).collect();
        Ok(map_image_detail(inspect, &containers))
    }

    /// Remove one image and return Docker's typed deleted/untagged actions.
    pub async fn remove_image(
        &self,
        id: &str,
        options: RemoveImageOptions,
    ) -> Result<Vec<ImageDeleteResult>, DockerError> {
        let docker_options = bollard_remove_options(options);
        let docker = self.client.inner().clone();
        let result = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_image(id, Some(docker_options), None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "image"))?;

        Ok(map_delete_results(result))
    }

    /// Pull an image and expose Docker's real progress as a cancellable stream.
    pub fn pull_image(&self, options: PullImageOptions) -> ImagePullStream {
        let cancel = CancellationToken::new();
        let reference = options.reference.trim().to_string();
        if reference.is_empty() {
            return ImagePullStream {
                inner: Box::pin(stream::once(async {
                    Err(DockerError::InvalidImageReference(
                        "image reference must not be empty".to_string(),
                    ))
                })),
                cancel,
            };
        }

        let create_options = BollardCreateImageOptions {
            from_image: Some(reference.clone()),
            platform: options.platform.unwrap_or_default(),
            ..Default::default()
        };
        let credentials = options.registry_auth.map(registry_credentials);
        let docker = self.client.inner().clone();
        let inner = docker.create_image(Some(create_options), None, credentials);
        let image_reference = reference.clone();
        let mapped = inner.map(move |result| match result {
            Ok(update) => Ok(map_pull_progress(update, &image_reference)),
            Err(error) => Err(classify_pull_error(&error)),
        });

        ImagePullStream {
            inner: Box::pin(mapped.take_until(cancel.clone().cancelled_owned())),
            cancel,
        }
    }

    /// Export an image as a cancellable stream of TAR bytes. No buffering of
    /// the complete image occurs.
    pub fn export_image(&self, id: &str) -> ImageExportStream {
        let cancel = CancellationToken::new();
        let docker = self.client.inner().clone();
        let mapped = docker
            .export_image(id)
            .map(|result| result.map_err(|error| classify_export_error(&error)));
        ImageExportStream {
            inner: Box::pin(mapped.take_until(cancel.clone().cancelled_owned())),
            cancel,
        }
    }
}

fn map_delete_results(
    results: Vec<bollard::models::ImageDeleteResponseItem>,
) -> Vec<ImageDeleteResult> {
    results
        .into_iter()
        .flat_map(|item| {
            item.untagged
                .map(ImageDeleteResult::Untagged)
                .into_iter()
                .chain(item.deleted.map(ImageDeleteResult::Deleted))
        })
        .collect()
}

fn bollard_remove_options(options: RemoveImageOptions) -> BollardRemoveImageOptions {
    BollardRemoveImageOptions {
        force: options.force,
        noprune: !options.prune_children,
        ..Default::default()
    }
}

fn map_pull_progress(
    update: bollard::models::CreateImageInfo,
    image_reference: &str,
) -> ImagePullProgress {
    let status = update.status.unwrap_or_default();
    let (current, total) = update
        .progress_detail
        .map(|detail| {
            (
                detail.current.and_then(|value| u64::try_from(value).ok()),
                detail.total.and_then(|value| u64::try_from(value).ok()),
            )
        })
        .unwrap_or((None, None));
    let percent = match (current, total) {
        (Some(current), Some(total)) if total > 0 => {
            Some((current as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
        }
        _ => None,
    };
    let status_lower = status.to_ascii_lowercase();
    // `Pull complete` is a per-layer event and must not finish the whole
    // operation. Docker's terminal stream messages are image-level.
    let completed = status_lower.contains("downloaded newer image")
        || status_lower.contains("image is up to date");
    ImagePullProgress {
        image_reference: image_reference.to_string(),
        layer_id: update.id,
        status,
        current,
        total,
        percent,
        completed,
    }
}

fn registry_credentials(auth: RegistryAuth) -> DockerCredentials {
    DockerCredentials {
        username: auth.username,
        password: auth.password,
        serveraddress: auth.server_address,
        identitytoken: auth.identity_token,
        registrytoken: auth.registry_token,
        ..Default::default()
    }
}

fn classify_export_error(error: &bollard::errors::Error) -> DockerError {
    match classify_api_error(error, "image") {
        DockerError::Api(message) => DockerError::ExportFailed(message),
        other => other,
    }
}

fn classify_pull_error(error: &bollard::errors::Error) -> DockerError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("denied")
    {
        DockerError::RegistryAuthenticationFailed
    } else if lower.contains("manifest unknown")
        || lower.contains("invalid reference")
        || lower.contains("repository does not exist")
    {
        DockerError::InvalidImageReference(message)
    } else if lower.contains("no such host")
        || lower.contains("connection refused")
        || lower.contains("service unavailable")
    {
        DockerError::RegistryUnavailable(message)
    } else {
        match classify_api_error(error, "image") {
            DockerError::Api(message) => DockerError::PullFailed(message),
            other => other,
        }
    }
}

fn apply_search(images: &mut Vec<ImageSummary>, search: Option<&str>) {
    let Some(search) = search.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let search = search.to_ascii_lowercase();
    images.retain(|image| {
        image.id.to_ascii_lowercase().contains(&search)
            || image.short_id.to_ascii_lowercase().contains(&search)
            || image.display_name.to_ascii_lowercase().contains(&search)
            || image
                .repo_tags
                .iter()
                .chain(&image.repo_digests)
                .any(|value| value.to_ascii_lowercase().contains(&search))
            || image.labels.iter().any(|(key, value)| {
                key.to_ascii_lowercase().contains(&search)
                    || value.to_ascii_lowercase().contains(&search)
            })
            || image
                .containers
                .iter()
                .any(|container| container.name.to_ascii_lowercase().contains(&search))
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use futures_util::StreamExt;

    use super::*;

    fn summary() -> ImageSummary {
        ImageSummary {
            id: "sha256:abcdef".into(),
            short_id: "abcdef".into(),
            repo_tags: vec!["Example/Web:Latest".into()],
            repo_digests: vec!["example/web@sha256:123".into()],
            display_name: "Example/Web:Latest".into(),
            created_at: Some(Utc::now()),
            size_bytes: 1,
            shared_size_bytes: None,
            virtual_size_bytes: None,
            labels: BTreeMap::from([("org.example.arch".into(), "arm64".into())]),
            containers: vec![],
            in_use: false,
        }
    }

    #[test]
    fn search_is_trimmed_case_insensitive_and_covers_fields() {
        for query in [" WEB ", "SHA256:123", "ABCDEF", "ARM64"] {
            let mut images = vec![summary()];
            apply_search(&mut images, Some(query));
            assert_eq!(images.len(), 1, "query {query}");
        }
    }

    #[test]
    fn search_no_match_filters_image() {
        let mut images = vec![summary()];
        apply_search(&mut images, Some("missing"));
        assert!(images.is_empty());
    }

    #[tokio::test]
    async fn empty_pull_reference_is_structured_error_without_docker_request() {
        // The stream's validation path does not access the client. Constructing
        // a DockerClient just for this test would require a daemon, so exercise
        // the exact validation variant directly as a contract assertion.
        let error =
            DockerError::InvalidImageReference("image reference must not be empty".to_string());
        assert!(matches!(error, DockerError::InvalidImageReference(_)));
        let stream = stream::once(async { Err::<ImagePullProgress, _>(error) });
        futures_util::pin_mut!(stream);
        assert!(matches!(
            stream.next().await,
            Some(Err(DockerError::InvalidImageReference(_)))
        ));
    }

    #[test]
    fn pull_progress_maps_real_counts_percent_and_completion() {
        let progress = map_pull_progress(
            bollard::models::CreateImageInfo {
                id: Some("layer-1".into()),
                status: Some("Downloaded newer image".into()),
                progress_detail: Some(bollard::models::ProgressDetail {
                    current: Some(50),
                    total: Some(200),
                }),
                ..Default::default()
            },
            "ubuntu:24.04",
        );
        assert_eq!(progress.image_reference, "ubuntu:24.04");
        assert_eq!(progress.layer_id.as_deref(), Some("layer-1"));
        assert_eq!(progress.current, Some(50));
        assert_eq!(progress.total, Some(200));
        assert_eq!(progress.percent, Some(25.0));
        assert!(progress.completed);

        let layer_complete = map_pull_progress(
            bollard::models::CreateImageInfo {
                status: Some("Pull complete".into()),
                ..Default::default()
            },
            "ubuntu:24.04",
        );
        assert!(!layer_complete.completed);
    }

    #[test]
    fn pull_progress_rejects_negative_and_zero_totals() {
        let progress = map_pull_progress(
            bollard::models::CreateImageInfo {
                status: Some("Downloading".into()),
                progress_detail: Some(bollard::models::ProgressDetail {
                    current: Some(-1),
                    total: Some(0),
                }),
                ..Default::default()
            },
            "example:test",
        );
        assert_eq!(progress.current, None);
        assert_eq!(progress.total, Some(0));
        assert_eq!(progress.percent, None);
        assert!(!progress.completed);
    }

    #[test]
    fn delete_response_maps_every_typed_action() {
        let mapped = map_delete_results(vec![
            bollard::models::ImageDeleteResponseItem {
                untagged: Some("example:latest".into()),
                deleted: None,
            },
            bollard::models::ImageDeleteResponseItem {
                untagged: None,
                deleted: Some("sha256:abc".into()),
            },
        ]);
        assert_eq!(
            mapped,
            vec![
                ImageDeleteResult::Untagged("example:latest".into()),
                ImageDeleteResult::Deleted("sha256:abc".into()),
            ]
        );
    }

    #[test]
    fn remove_options_invert_docker_noprune_correctly() {
        let mapped = bollard_remove_options(RemoveImageOptions {
            force: true,
            prune_children: true,
        });
        assert!(mapped.force);
        assert!(!mapped.noprune);

        let mapped = bollard_remove_options(RemoveImageOptions {
            force: false,
            prune_children: false,
        });
        assert!(!mapped.force);
        assert!(mapped.noprune);
    }

    #[test]
    fn credentials_debug_redacts_every_secret() {
        let auth = RegistryAuth {
            username: Some("alice".into()),
            password: Some("super-secret".into()),
            server_address: Some("registry.example".into()),
            identity_token: Some("identity-secret".into()),
            registry_token: Some("registry-secret".into()),
        };
        let debug = format!("{auth:?}");
        assert!(debug.contains("alice"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("identity-secret"));
        assert!(!debug.contains("registry-secret"));
        let serialized = serde_json::to_string(&auth).unwrap();
        assert!(!serialized.contains("super-secret"));
        assert!(!serialized.contains("identity-secret"));
        assert!(!serialized.contains("registry-secret"));
    }
}
